//! Architecture guard — `DerivedRawState.import_routes` and
//! `DerivedRawState.import_routes_known_miss_recorded_at_generation`
//! producer ownership.
//!
//! Block 1.E enforces the route-cache invariant in two parts:
//!
//! 1. **Strict guard on the known-miss generation sidecar.**
//!    `DerivedRawState.import_routes_known_miss_recorded_at_generation`
//!    is the field that records the workspace `content_generation` at
//!    which a known-miss specifier (no resolved canonical, no
//!    candidates, no effective target) was admitted. Its admission is
//!    single-producer: only [`VerterHost::set_import_dependencies`]
//!    may insert, extend, or assign entries. Lifecycle reset methods
//!    ([`VerterHost::configure_projects`] and
//!    `VerterHost::finish_upsert_post_commit`) may `clear()`
//!    the field, but never produce new entries. Any other writer is a
//!    correctness defect: extending the sidecar from a non-snapshot
//!    producer risks stamping a stale known-miss with a fresh
//!    generation, which would extend a negative answer that should
//!    have re-resolved.
//!
//! 2. **Allow-listed guard on the positive route map.**
//!    `DerivedRawState.import_routes` admits two operation classes:
//!    full caller-supplied snapshot replacement
//!    ([`VerterHost::set_import_dependencies`]) and single positive
//!    route publication
//!    ([`VerterHost::cache_positive_import_route_result`]). Lifecycle
//!    reset methods may `clear()`. Any other mutating method
//!    (`insert`, `remove`, `retain`, `extend`, `drain`, `append`, or
//!    direct assignment) on a `DerivedRawState.import_routes` binding
//!    outside this allow-list is rejected.
//!
//! The guard scans `crates/verter_session/src/**/*.rs` excluding
//! sibling `*_tests.rs` files. It does **not** flag reads of
//! `entry.import_routes` (e.g. cloning into a snapshot, iterating to
//! build dep targets) — only mutations. It tracks local variables
//! bound from `self.derived_raw_cache().entry(_).or_default()` /
//! `self.derived_raw_cache().get_mut(_)` /
//! `self.derived_raw_cache().iter_mut()` so the guard targets
//! `DerivedRawState` mutations and does not flag unrelated
//! `StoreView.import_routes` / `IndexedReady.import_routes`
//! mutations.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use syn::visit::Visit;
use syn::{
    Attribute, Block, Expr, ExprAssign, ExprCall, ExprMethodCall, ImplItemFn, ItemFn, ItemImpl,
    ItemMod, Local, Meta, Pat,
};
use walkdir::WalkDir;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("repo root")
        .to_path_buf()
}

#[derive(Debug)]
struct Violation {
    file: PathBuf,
    enclosing_fn: String,
    op: String,
    detail: String,
}

impl std::fmt::Display for Violation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}: in fn `{}`: {} -- {}",
            self.file.display(),
            self.enclosing_fn,
            self.op,
            self.detail
        )
    }
}

/// Whitelisted writer methods for the positive route map.
const ROUTE_MAP_ADMIT_ALLOWED_FNS: &[&str] = &[
    "set_import_dependencies",
    "cache_positive_import_route_result",
];

/// Whitelisted reset methods for the positive route map.
const ROUTE_MAP_CLEAR_ALLOWED_FNS: &[&str] = &["configure_projects", "finish_upsert_post_commit"];

/// The only writer allowed to admit known-miss sidecar entries
/// (snapshot assignment).
const SIDECAR_ASSIGN_ALLOWED_FNS: &[&str] = &["set_import_dependencies"];

/// Whitelisted reset methods for the known-miss sidecar.
const SIDECAR_CLEAR_ALLOWED_FNS: &[&str] = &["configure_projects", "finish_upsert_post_commit"];

/// Visitor that tracks the current enclosing `fn` name and the local
/// variables in scope that are bound from
/// `self.derived_raw_cache().{entry,get_mut,iter_mut}`. Inside any
/// such binding, `.import_routes` / `.value_mut().import_routes` /
/// `.import_routes_known_miss_recorded_at_generation` is treated as
/// the guarded `DerivedRawState` field. Reads (e.g. `.import_routes`
/// on the right-hand side of an assignment, `.import_routes.iter()`,
/// `.import_routes.clone()`) are not flagged.
struct Scanner<'a> {
    file: &'a Path,
    fn_stack: Vec<String>,
    /// Stack of binding sets, one per `Block`/`ItemFn`/`ImplItemFn`.
    /// Each frame is the set of local idents bound from
    /// `derived_raw_cache()` in that scope.
    binding_scopes: Vec<HashSet<String>>,
    /// Depth-counter for `#[cfg(test)]` items: positive depth means
    /// the visitor is currently inside a test-gated `fn`/`mod`/`impl`.
    /// `host_test_seed.rs` is entirely under `#[cfg(test)] impl
    /// VerterHost` so its `import_routes` seeding is structural test
    /// scaffolding, not a production producer.
    cfg_test_depth: u32,
    violations: &'a mut Vec<Violation>,
}

impl<'a> Scanner<'a> {
    fn new(file: &'a Path, violations: &'a mut Vec<Violation>) -> Self {
        Self {
            file,
            fn_stack: Vec::new(),
            binding_scopes: vec![HashSet::new()],
            cfg_test_depth: 0,
            violations,
        }
    }

    fn current_fn(&self) -> &str {
        self.fn_stack.last().map(String::as_str).unwrap_or("<root>")
    }

    fn binding_in_scope(&self, ident: &str) -> bool {
        self.binding_scopes.iter().any(|s| s.contains(ident))
    }

    fn record(&mut self, op: &str, detail: String) {
        if self.cfg_test_depth > 0 {
            return;
        }
        self.violations.push(Violation {
            file: self.file.to_path_buf(),
            enclosing_fn: self.current_fn().to_string(),
            op: op.to_string(),
            detail,
        });
    }

    /// Recognise an expression that has type `DashMap<_,DerivedRawState>::Entry`
    /// or `RefMut<DerivedRawState>` at the call site. We use a
    /// syntactic approximation: a chain ending in
    /// `self.derived_raw_cache().entry(_).or_default()`,
    /// `.derived_raw_cache().get_mut(_)`, or
    /// `.derived_raw_cache().iter_mut()`.
    ///
    /// Walks the expression to see whether any sub-call resolves to
    /// `derived_raw_cache()`. Bare `derived.value_mut()` chains
    /// captured via a local binding rely on `binding_in_scope`
    /// instead.
    fn is_derived_raw_cache_binding_source(expr: &Expr) -> bool {
        struct Probe {
            saw_derived_raw_cache: bool,
        }
        impl<'ast> Visit<'ast> for Probe {
            fn visit_expr_method_call(&mut self, e: &'ast ExprMethodCall) {
                if e.method == "derived_raw_cache" {
                    self.saw_derived_raw_cache = true;
                }
                syn::visit::visit_expr_method_call(self, e);
            }
            fn visit_expr_call(&mut self, e: &'ast ExprCall) {
                syn::visit::visit_expr_call(self, e);
            }
        }
        let mut p = Probe {
            saw_derived_raw_cache: false,
        };
        p.visit_expr(expr);
        p.saw_derived_raw_cache
    }

    /// Walk an expression to determine if its base receiver is a
    /// `DerivedRawState`-binding (either a local in scope, or the
    /// inline result of `self.derived_raw_cache().{entry,get_mut,
    /// iter_mut}(...).{or_default,value_mut}()`).
    ///
    /// Receivers that pass:
    ///   * `derived`, `derived_ref`, `entry`, `dr` ... where the
    ///     ident is in a tracked binding scope;
    ///   * any chain whose innermost method-call walks to
    ///     `derived_raw_cache()`;
    ///   * `derived.value_mut()` / `entry.value_mut()` (where
    ///     `derived` / `entry` is in scope).
    fn receiver_is_derived_raw_state(&self, expr: &Expr) -> bool {
        match expr {
            Expr::Path(p) => {
                if let Some(ident) = p.path.get_ident() {
                    self.binding_in_scope(&ident.to_string())
                } else {
                    false
                }
            }
            Expr::MethodCall(m) => {
                // `<recv>.value_mut()` / `<recv>.deref_mut()` etc. —
                // a wrapper method; recurse on receiver.
                if matches!(
                    m.method.to_string().as_str(),
                    "value_mut" | "value" | "deref_mut" | "deref" | "as_mut" | "as_ref" | "get_mut"
                ) && self.receiver_is_derived_raw_state(&m.receiver)
                {
                    return true;
                }
                Self::is_derived_raw_cache_binding_source(expr)
            }
            Expr::Field(f) => self.receiver_is_derived_raw_state(&f.base),
            Expr::Paren(p) => self.receiver_is_derived_raw_state(&p.expr),
            Expr::Reference(r) => self.receiver_is_derived_raw_state(&r.expr),
            _ => Self::is_derived_raw_cache_binding_source(expr),
        }
    }

    /// If `expr` is a `MethodCall` whose method is `entry` /
    /// `iter_mut` / `values_mut` (the entry-chain entry points) and
    /// whose receiver is a `Field` access on a guarded
    /// `DerivedRawState` field, return that field plus the inner
    /// method name. Wrapper methods that sit between the chain and
    /// the field expression (`value_mut`, `deref_mut`, etc.) are
    /// transparently peeled so a chain like
    /// `derived.value_mut().import_routes.entry(_).or_insert(_)`
    /// is matched.
    fn entry_chain_field(&self, expr: &Expr) -> Option<(GuardedField, String)> {
        let inner = match expr {
            Expr::MethodCall(m) => m,
            Expr::Paren(p) => return self.entry_chain_field(&p.expr),
            _ => return None,
        };
        let inner_method = inner.method.to_string();
        if !matches!(inner_method.as_str(), "entry" | "iter_mut" | "values_mut") {
            return None;
        }
        // Peel parentheses off the inner receiver until we reach a
        // `Field` (or run out of wrappers).
        let mut base = &*inner.receiver;
        loop {
            match base {
                Expr::Field(recv_field) => {
                    let field_name = match &recv_field.member {
                        syn::Member::Named(i) => i.to_string(),
                        syn::Member::Unnamed(_) => return None,
                    };
                    let guarded = match field_name.as_str() {
                        "import_routes" => GuardedField::Routes,
                        "import_routes_known_miss_recorded_at_generation" => {
                            GuardedField::SidecarKnownMissGeneration
                        }
                        _ => return None,
                    };
                    if self.receiver_is_derived_raw_state(&recv_field.base) {
                        return Some((guarded, inner_method));
                    }
                    return None;
                }
                Expr::Paren(p) => base = &p.expr,
                _ => return None,
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum GuardedField {
    Routes,
    SidecarKnownMissGeneration,
}

impl<'ast> Visit<'ast> for Scanner<'_> {
    fn visit_item_mod(&mut self, m: &'ast ItemMod) {
        // Track `#[cfg(test)] mod ...` (and `mod tests { ... }`) so
        // nested writers inside test-only modules are skipped.
        let entered_test = has_cfg_test(&m.attrs) || m.ident == "tests";
        if entered_test {
            self.cfg_test_depth += 1;
        }
        syn::visit::visit_item_mod(self, m);
        if entered_test {
            self.cfg_test_depth -= 1;
        }
    }

    fn visit_item_impl(&mut self, i: &'ast ItemImpl) {
        // `#[cfg(test)] impl VerterHost { ... }` — `host_test_seed.rs`
        // uses this shape to gate test-seeding helpers.
        let entered_test = has_cfg_test(&i.attrs);
        if entered_test {
            self.cfg_test_depth += 1;
        }
        syn::visit::visit_item_impl(self, i);
        if entered_test {
            self.cfg_test_depth -= 1;
        }
    }

    fn visit_item_fn(&mut self, f: &'ast ItemFn) {
        let entered_test = has_cfg_test(&f.attrs);
        if entered_test {
            self.cfg_test_depth += 1;
        }
        self.fn_stack.push(f.sig.ident.to_string());
        self.binding_scopes.push(HashSet::new());
        syn::visit::visit_item_fn(self, f);
        self.binding_scopes.pop();
        self.fn_stack.pop();
        if entered_test {
            self.cfg_test_depth -= 1;
        }
    }

    fn visit_impl_item_fn(&mut self, f: &'ast ImplItemFn) {
        let entered_test = has_cfg_test(&f.attrs);
        if entered_test {
            self.cfg_test_depth += 1;
        }
        self.fn_stack.push(f.sig.ident.to_string());
        self.binding_scopes.push(HashSet::new());
        syn::visit::visit_impl_item_fn(self, f);
        self.binding_scopes.pop();
        self.fn_stack.pop();
        if entered_test {
            self.cfg_test_depth -= 1;
        }
    }

    fn visit_block(&mut self, b: &'ast Block) {
        self.binding_scopes.push(HashSet::new());
        syn::visit::visit_block(self, b);
        self.binding_scopes.pop();
    }

    fn visit_local(&mut self, l: &'ast Local) {
        // `let <pat> = <init>;` — if the initializer resolves to a
        // `derived_raw_cache()` binding source, register the bound
        // ident in the current scope.
        if let Some(init) = &l.init {
            if Self::is_derived_raw_cache_binding_source(&init.expr) {
                if let Pat::Ident(pat_ident) = &l.pat {
                    if let Some(top) = self.binding_scopes.last_mut() {
                        top.insert(pat_ident.ident.to_string());
                    }
                }
            }
        }
        syn::visit::visit_local(self, l);
    }

    fn visit_expr_for_loop(&mut self, f: &'ast syn::ExprForLoop) {
        // `for <pat> in self.derived_raw_cache().iter_mut() { ... }`
        // binds `<pat>` to a `DerivedRawState`-mutable view.
        let loop_binding = if Self::is_derived_raw_cache_binding_source(&f.expr) {
            if let Pat::Ident(pat_ident) = &*f.pat {
                Some(pat_ident.ident.to_string())
            } else {
                None
            }
        } else {
            None
        };
        self.binding_scopes.push(HashSet::new());
        if let Some(ident) = loop_binding {
            if let Some(top) = self.binding_scopes.last_mut() {
                top.insert(ident);
            }
        }
        syn::visit::visit_expr_for_loop(self, f);
        self.binding_scopes.pop();
    }

    fn visit_expr_assign(&mut self, a: &'ast ExprAssign) {
        // Detect: `<recv>.import_routes = ...` /
        // `<recv>.value_mut().import_routes = ...` /
        // `<recv>.import_routes_known_miss_recorded_at_generation = ...`.
        if let Expr::Field(lhs_field) = &*a.left {
            let field_name = match &lhs_field.member {
                syn::Member::Named(i) => i.to_string(),
                syn::Member::Unnamed(_) => String::new(),
            };
            let guarded = match field_name.as_str() {
                "import_routes" => Some(GuardedField::Routes),
                "import_routes_known_miss_recorded_at_generation" => {
                    Some(GuardedField::SidecarKnownMissGeneration)
                }
                _ => None,
            };
            if let Some(field) = guarded {
                if self.receiver_is_derived_raw_state(&lhs_field.base) {
                    let op = match field {
                        GuardedField::Routes => "import_routes := <expr>",
                        GuardedField::SidecarKnownMissGeneration => {
                            "import_routes_known_miss_recorded_at_generation := <expr>"
                        }
                    };
                    let detail = format!(
                        "writer `{}` assigned `{}` (allowed only in: {})",
                        self.current_fn(),
                        op,
                        match field {
                            GuardedField::Routes => ROUTE_MAP_ADMIT_ALLOWED_FNS.join(", "),
                            GuardedField::SidecarKnownMissGeneration =>
                                SIDECAR_ASSIGN_ALLOWED_FNS.join(", "),
                        }
                    );
                    let allowed = match field {
                        GuardedField::Routes => {
                            ROUTE_MAP_ADMIT_ALLOWED_FNS.contains(&self.current_fn())
                        }
                        GuardedField::SidecarKnownMissGeneration => {
                            SIDECAR_ASSIGN_ALLOWED_FNS.contains(&self.current_fn())
                        }
                    };
                    if !allowed {
                        self.record(op, detail);
                    }
                }
            }
        }
        syn::visit::visit_expr_assign(self, a);
    }

    fn visit_expr_method_call(&mut self, m: &'ast ExprMethodCall) {
        // Detect mutating method-call on `<recv>.import_routes` /
        // `<recv>.import_routes_known_miss_recorded_at_generation`.
        //
        // Two shapes are caught:
        //
        //   1. **Direct method-call on the field expression** —
        //      `<recv>.import_routes.<method>(...)`. The receiver of
        //      `m` is the FIELD expression itself. Mutating methods
        //      (`insert`, `clear`, `remove`, `retain`, `extend`,
        //      `drain`, `append`, `entry`, `iter_mut`, `values_mut`,
        //      `get_mut`) are gated; truly read-only accessors
        //      (`get`, `iter`, `values`, `keys`, `len`, `is_empty`,
        //      `contains_key`, `clone`) are passthrough.
        //
        //   2. **Mutating accessor chained off a `.entry(...)` /
        //      `.iter_mut()` / `.values_mut()` on the field** —
        //      e.g. `<recv>.import_routes.entry(_).or_insert(_)`.
        //      Without explicit chain-handling, the outer method
        //      call's receiver is a `MethodCall` (not a `Field`), so
        //      the direct-shape check above misses the outer call.
        //      The inner `.entry(...)` is already caught by shape (1)
        //      because `.entry()` is treated as a mutating accessor
        //      on the field (an Entry hands out `&mut V`); the
        //      explicit chain rule below provides a discriminating
        //      message and defends against a future relaxation of
        //      shape (1).
        if let Expr::Field(recv_field) = &*m.receiver {
            let field_name = match &recv_field.member {
                syn::Member::Named(i) => i.to_string(),
                syn::Member::Unnamed(_) => String::new(),
            };
            let guarded = match field_name.as_str() {
                "import_routes" => Some(GuardedField::Routes),
                "import_routes_known_miss_recorded_at_generation" => {
                    Some(GuardedField::SidecarKnownMissGeneration)
                }
                _ => None,
            };
            if let Some(field) = guarded {
                if self.receiver_is_derived_raw_state(&recv_field.base) {
                    let method = m.method.to_string();
                    let cur = self.current_fn().to_string();
                    let allowed = match (field, method.as_str()) {
                        // Positive route admission: `.insert(_)` permitted only
                        // in the renamed positive-route producer (plus the full
                        // snapshot writer which uses direct assignment, not
                        // `.insert`, but we accept either form there).
                        (GuardedField::Routes, "insert") => {
                            ROUTE_MAP_ADMIT_ALLOWED_FNS.contains(&cur.as_str())
                        }
                        // Lifecycle reset: `.clear()` permitted only in the
                        // two lifecycle reset methods.
                        (GuardedField::Routes, "clear") => {
                            ROUTE_MAP_CLEAR_ALLOWED_FNS.contains(&cur.as_str())
                        }
                        (GuardedField::SidecarKnownMissGeneration, "clear") => {
                            SIDECAR_CLEAR_ALLOWED_FNS.contains(&cur.as_str())
                        }
                        // Mutating map accessors that hand out a
                        // mutable handle on a value (or build one
                        // lazily via `Entry::or_*`). `.entry()`
                        // returns an `Entry` whose `.or_insert*` /
                        // `.or_default` / `.and_modify` paths perform
                        // an admission; `.iter_mut()` / `.values_mut()`
                        // / `.get_mut()` hand out `&mut V` which
                        // permits arbitrary out-of-band mutation.
                        // Restricted to the admission allow-list for
                        // the positive route map; never allowed on
                        // the strict sidecar (the sidecar admits only
                        // via direct assignment in
                        // `set_import_dependencies`).
                        (GuardedField::Routes, "entry" | "iter_mut" | "values_mut" | "get_mut") => {
                            ROUTE_MAP_ADMIT_ALLOWED_FNS.contains(&cur.as_str())
                        }
                        // Truly read-only operations — never flagged.
                        (
                            _,
                            "get" | "iter" | "values" | "keys" | "len" | "is_empty"
                            | "contains_key" | "clone",
                        ) => true,
                        // Everything else on the guarded fields is rejected.
                        _ => false,
                    };
                    if !allowed {
                        let detail = format!(
                            "writer `{cur}` called `.{method}(...)` on `{}` (admission allowed: {}; clear allowed: {})",
                            match field {
                                GuardedField::Routes => "import_routes",
                                GuardedField::SidecarKnownMissGeneration =>
                                    "import_routes_known_miss_recorded_at_generation",
                            },
                            match field {
                                GuardedField::Routes => ROUTE_MAP_ADMIT_ALLOWED_FNS.join(", "),
                                GuardedField::SidecarKnownMissGeneration =>
                                    SIDECAR_ASSIGN_ALLOWED_FNS.join(", "),
                            },
                            match field {
                                GuardedField::Routes => ROUTE_MAP_CLEAR_ALLOWED_FNS.join(", "),
                                GuardedField::SidecarKnownMissGeneration =>
                                    SIDECAR_CLEAR_ALLOWED_FNS.join(", "),
                            },
                        );
                        self.record(&format!(".{method}()"), detail);
                    }
                }
            }
        }
        // Shape (2): mutating-chain rule. The outer method call
        // (`or_insert` / `or_insert_with` / `or_insert_with_key` /
        // `or_default` / `and_modify`) consumes the `Entry` returned
        // by `.entry()` on a guarded field. The inner `.entry()` is
        // already caught by shape (1), but the chain rule pins the
        // out-of-band mutation explicitly so a future relaxation of
        // `.entry()` (e.g. allowing it for a probe-style read that
        // intentionally never mutates) cannot silently re-open the
        // mutating chain for arbitrary callers.
        const ENTRY_MUTATING_CHAINS: &[&str] = &[
            "or_insert",
            "or_insert_with",
            "or_insert_with_key",
            "or_default",
            "and_modify",
        ];
        let outer_method = m.method.to_string();
        if ENTRY_MUTATING_CHAINS.contains(&outer_method.as_str()) {
            if let Some((field, inner_method)) = self.entry_chain_field(&m.receiver) {
                let cur = self.current_fn().to_string();
                let allowed = match field {
                    GuardedField::Routes => ROUTE_MAP_ADMIT_ALLOWED_FNS.contains(&cur.as_str()),
                    // The strict sidecar has no entry-chain producer.
                    // Snapshot admission is direct assignment in
                    // `set_import_dependencies`; nothing else may
                    // touch it via `.entry(_).or_*`.
                    GuardedField::SidecarKnownMissGeneration => false,
                };
                if !allowed {
                    let detail = format!(
                        "writer `{cur}` chained `.{outer_method}(...)` after `.{inner_method}(...)` on `{}` \
                         (admission allowed: {}); mutating-map accessors must route through the canonical admission helper",
                        match field {
                            GuardedField::Routes => "import_routes",
                            GuardedField::SidecarKnownMissGeneration =>
                                "import_routes_known_miss_recorded_at_generation",
                        },
                        match field {
                            GuardedField::Routes => ROUTE_MAP_ADMIT_ALLOWED_FNS.join(", "),
                            GuardedField::SidecarKnownMissGeneration =>
                                SIDECAR_ASSIGN_ALLOWED_FNS.join(", "),
                        },
                    );
                    self.record(&format!(".{inner_method}().{outer_method}()"), detail);
                }
            }
        }
        syn::visit::visit_expr_method_call(self, m);
    }
}

/// True if any of the attributes is `#[cfg(test)]`,
/// `#[cfg(any(test, ...))]`, or `#[cfg(all(..., test, ...))]`. Mirrors
/// the `has_cfg_test` helper in `architecture_guards.rs` so existing
/// cfg-test cases (e.g. `host_test_seed.rs::seed_indexed_ready_for_test`)
/// are uniformly recognised as test-only.
fn has_cfg_test(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|a| {
        if !a.path().is_ident("cfg") {
            return false;
        }
        let rendered = match &a.meta {
            Meta::List(list) => list.tokens.to_string(),
            _ => return false,
        };
        for token in rendered.split(|c: char| !c.is_alphanumeric() && c != '_') {
            if token == "test" {
                return true;
            }
        }
        false
    })
}

fn walk_production_rs_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for entry in WalkDir::new(root).into_iter().filter_map(Result::ok) {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let p = path.to_string_lossy().replace('\\', "/");
        // Skip integration tests under tests/, benches/, examples/,
        // target/, and sibling *_tests.rs files (test fixtures
        // legitimately seed `import_routes` via direct entry
        // mutation).
        if p.contains("/tests/") || p.contains("/benches/") || p.contains("/examples/") {
            continue;
        }
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if name.ends_with("_tests.rs") || name == "tests.rs" {
                continue;
            }
        }
        files.push(path.to_path_buf());
    }
    files
}

fn scan_file(path: &Path, violations: &mut Vec<Violation>) {
    let src =
        std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
    let parsed =
        syn::parse_file(&src).unwrap_or_else(|e| panic!("parse {}: {}", path.display(), e));
    let mut scanner = Scanner::new(path, violations);
    scanner.visit_file(&parsed);
}

fn format_violations(violations: &[Violation]) -> String {
    let mut by_file: BTreeMap<&Path, Vec<&Violation>> = BTreeMap::new();
    for v in violations {
        by_file.entry(v.file.as_path()).or_default().push(v);
    }
    let mut lines = Vec::new();
    for (file, vs) in by_file {
        lines.push(format!("  {}", file.display()));
        for v in vs {
            lines.push(format!(
                "    fn `{}`: {} -- {}",
                v.enclosing_fn, v.op, v.detail
            ));
        }
    }
    format!(
        "found {} import-route writer violation(s):\n{}",
        violations.len(),
        lines.join("\n")
    )
}

// ---------------------------------------------------------------------------
// Guard 1: positive-route admission allow-list.
// ---------------------------------------------------------------------------

/// Block 1.E positive-route admission allow-list.
///
/// `DerivedRawState.import_routes` may be mutated only by:
///   * `VerterHost::set_import_dependencies` — full snapshot writer;
///   * `VerterHost::cache_positive_import_route_result` — positive-only
///     point admission;
///   * `VerterHost::configure_projects` — lifecycle clear;
///   * `VerterHost::finish_upsert_post_commit` — lifecycle clear.
///
/// Any other writer flagged here means a new admission path was added
/// without being routed through the canonical helper. The fix is to
/// either call `cache_positive_import_route_result` (positive-only) or
/// `set_import_dependencies` (full snapshot with known-miss admission).
#[test]
fn import_routes_writer_allow_list() {
    let crate_root = workspace_root().join("crates/verter_session/src");
    let mut violations = Vec::new();
    for file in walk_production_rs_files(&crate_root) {
        scan_file(&file, &mut violations);
    }
    // Filter down to `import_routes` violations only (Guard 2 owns the
    // sidecar half).
    let route_violations: Vec<_> = violations
        .into_iter()
        .filter(|v| {
            !v.op
                .contains("import_routes_known_miss_recorded_at_generation")
                && !v
                    .op
                    .contains(".clear() on `import_routes_known_miss_recorded_at_generation")
                && !v
                    .detail
                    .contains("on `import_routes_known_miss_recorded_at_generation`")
        })
        .collect();
    assert!(
        route_violations.is_empty(),
        "Block 1.E `import_routes_writer_allow_list` violation:\n{}\n\n\
         `DerivedRawState.import_routes` is a two-mode admission field.\n\
           * Positive route point admission: `cache_positive_import_route_result`.\n\
           * Full caller-supplied snapshot: `set_import_dependencies`.\n\
           * Lifecycle clear: `configure_projects`, `finish_upsert_post_commit`.\n\
         If you need to publish a host-resolved positive route from a\n\
         new caller, route through `cache_positive_import_route_result`.\n\
         If you need to publish a bundler-supplied snapshot with known\n\
         misses, route through `set_import_dependencies`. Do not add a\n\
         new direct producer.",
        format_violations(&route_violations)
    );
}

// ---------------------------------------------------------------------------
// Guard 2: strict known-miss sidecar — single producer.
// ---------------------------------------------------------------------------

/// Block 1.E strict guard on the known-miss generation sidecar.
///
/// `DerivedRawState.import_routes_known_miss_recorded_at_generation`
/// admission is single-producer. The full-snapshot writer
/// (`set_import_dependencies`) computes the per-specifier generation
/// table from the caller's snapshot and assigns the whole map. The
/// two lifecycle reset methods (`configure_projects` and
/// `finish_upsert_post_commit`) may `.clear()` the sidecar
/// alongside `import_routes`. No other writer is allowed —
/// admitting from a non-snapshot producer would re-stamp a known
/// miss at the current `content_generation` and incorrectly extend a
/// stale negative answer that should have re-resolved.
#[test]
fn known_miss_generation_sidecar_strict() {
    let crate_root = workspace_root().join("crates/verter_session/src");
    let mut violations = Vec::new();
    for file in walk_production_rs_files(&crate_root) {
        scan_file(&file, &mut violations);
    }
    let sidecar_violations: Vec<_> = violations
        .into_iter()
        .filter(|v| {
            v.op.contains("import_routes_known_miss_recorded_at_generation")
                || v.detail
                    .contains("on `import_routes_known_miss_recorded_at_generation`")
                || v.detail
                    .contains("`import_routes_known_miss_recorded_at_generation`")
        })
        .collect();
    assert!(
        sidecar_violations.is_empty(),
        "Block 1.E `known_miss_generation_sidecar_strict` violation:\n{}\n\n\
         `DerivedRawState.import_routes_known_miss_recorded_at_generation`\n\
         is single-producer. Snapshot assignment only inside\n\
         `set_import_dependencies`; `.clear()` only inside\n\
         `configure_projects` and `finish_upsert_post_commit`.\n\
         Any other writer admits a known-miss generation stamp from a\n\
         non-snapshot producer, which incorrectly extends a stale\n\
         negative answer. If you need to admit known misses, route\n\
         through `set_import_dependencies`; positive-only point\n\
         admissions must use `cache_positive_import_route_result`\n\
         (which never touches the sidecar).",
        format_violations(&sidecar_violations)
    );
}

// ---------------------------------------------------------------------------
// Guard 3: positive-route helper sentinel — discriminating identity
// ---------------------------------------------------------------------------

/// Block 1.E sentinel — the renamed positive-route producer must
/// continue to exist with the correct shape. If a future refactor
/// either deletes `cache_positive_import_route_result` (folding it
/// into another method) or weakens its body to no longer construct a
/// positive `DependencyResolution`, this guard fails.
///
/// Discriminating property: the helper exists, lives in
/// `host_resolve/dependency_resolution.rs`, has a body that
/// constructs `resolved_canonical_id: Some(...)` and a non-empty
/// `possible_canonical_ids` vector, and does NOT mention
/// `import_routes_known_miss_recorded_at_generation`.
#[test]
fn positive_route_helper_shape() {
    let path =
        workspace_root().join("crates/verter_session/src/host_resolve/dependency_resolution.rs");
    let src =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
    let parsed =
        syn::parse_file(&src).unwrap_or_else(|e| panic!("parse {}: {}", path.display(), e));

    let mut found = false;
    let mut has_some_resolved = false;
    let mut has_candidate_vec = false;
    let mut writes_sidecar = false;

    for item in &parsed.items {
        if let syn::Item::Impl(item_impl) = item {
            collect_positive_helper(item_impl, &mut |body_src: &str| {
                found = true;
                if body_src.contains("resolved_canonical_id : Some")
                    || body_src.contains("resolved_canonical_id: Some")
                {
                    has_some_resolved = true;
                }
                if body_src.contains("possible_canonical_ids : vec !")
                    || body_src.contains("possible_canonical_ids: vec!")
                {
                    has_candidate_vec = true;
                }
                if body_src.contains("import_routes_known_miss_recorded_at_generation") {
                    writes_sidecar = true;
                }
            });
        }
    }

    assert!(
        found,
        "expected `cache_positive_import_route_result` to exist on `impl VerterHost` \
         in `crates/verter_session/src/host_resolve/dependency_resolution.rs`; \
         the helper is the canonical positive-route producer for Block 1.E"
    );
    assert!(
        has_some_resolved,
        "`cache_positive_import_route_result` must construct a positive \
         `DependencyResolution` with `resolved_canonical_id: Some(...)` — \
         a positive admission must carry a resolved canonical"
    );
    assert!(
        has_candidate_vec,
        "`cache_positive_import_route_result` must populate \
         `possible_canonical_ids: vec![...]` with at least one candidate — \
         an empty candidate list with no resolved id is a known-miss \
         shape and would route through `set_import_dependencies` instead"
    );
    assert!(
        !writes_sidecar,
        "`cache_positive_import_route_result` must NOT reference \
         `import_routes_known_miss_recorded_at_generation`; the positive \
         producer is sidecar-free by design"
    );
}

fn collect_positive_helper(item_impl: &ItemImpl, mut on_body: impl FnMut(&str)) {
    let receiver_is_verter_host = match &*item_impl.self_ty {
        syn::Type::Path(tp) => tp
            .path
            .segments
            .last()
            .map(|s| s.ident == "VerterHost")
            .unwrap_or(false),
        _ => false,
    };
    if !receiver_is_verter_host {
        return;
    }
    for item in &item_impl.items {
        if let syn::ImplItem::Fn(f) = item {
            if f.sig.ident == "cache_positive_import_route_result" {
                let body = quote_block_source(&f.block);
                on_body(&body);
            }
        }
    }
}

fn quote_block_source(block: &Block) -> String {
    use quote::ToTokens;
    block.to_token_stream().to_string()
}

// ---------------------------------------------------------------------------
// Guard 4: lifecycle reset symmetry sentinel.
// ---------------------------------------------------------------------------

/// `configure_projects` and `finish_upsert_post_commit` are the two
/// lifecycle reset producers. Both clear `import_routes` AND the
/// known-miss generation sidecar — leaving the sidecar populated after
/// either reset would carry a stale generation stamp into the next
/// admission cycle. This sentinel asserts the symmetry directly.
///
/// `finish_upsert_post_commit` is the per-canonical post-commit step of
/// the shared upsert engine (`upsert_many_with_priority`); the §6c
/// cutover moved the owner-source-update own-cache drain there from the
/// retired `upsert_via_scheduler_with_priority`, so the route + sidecar
/// clears now live in this function.
///
/// Discriminating property: searching the function bodies textually
/// for both `import_routes.clear()` and
/// `import_routes_known_miss_recorded_at_generation` ... `.clear()`.
/// If a future refactor drops either clear, this test fails.
#[test]
fn lifecycle_reset_clears_both_route_and_sidecar() {
    let lifecycle = workspace_root().join("crates/verter_session/src/host_lifecycle.rs");
    let upsert = workspace_root().join("crates/verter_session/src/host_upsert.rs");

    let lifecycle_src = std::fs::read_to_string(&lifecycle)
        .unwrap_or_else(|e| panic!("read {}: {}", lifecycle.display(), e));
    let upsert_src = std::fs::read_to_string(&upsert)
        .unwrap_or_else(|e| panic!("read {}: {}", upsert.display(), e));

    let lifecycle_parsed = syn::parse_file(&lifecycle_src).expect("parse host_lifecycle.rs");
    let upsert_parsed = syn::parse_file(&upsert_src).expect("parse host_upsert.rs");

    let cp_body = find_fn_body(&lifecycle_parsed, "configure_projects")
        .expect("`configure_projects` must exist in host_lifecycle.rs");
    let ups_body = find_fn_body(&upsert_parsed, "finish_upsert_post_commit").expect(
        "`finish_upsert_post_commit` (the shared upsert engine's per-canonical \
         post-commit step) must exist in host_upsert.rs",
    );

    // `quote::ToTokens` renders the body as space-separated tokens,
    // so `entry.import_routes.clear()` becomes
    // `entry . import_routes . clear ()`. Match the tokenised form.
    assert!(
        cp_body.contains("import_routes . clear ("),
        "`configure_projects` must clear `import_routes` during project resolver \
         reconfiguration; got body:\n{cp_body}"
    );
    assert!(
        cp_body.contains("import_routes_known_miss_recorded_at_generation . clear ("),
        "Block 1.E lifecycle symmetry: `configure_projects` must ALSO clear \
         `import_routes_known_miss_recorded_at_generation` to keep the known-miss \
         sidecar in lockstep with `import_routes` on project-graph reset. \
         Without the sidecar clear, a stale `content_generation` stamp survives \
         the reset and would suppress re-resolution after the next admission. \
         Body did not contain `import_routes_known_miss_recorded_at_generation . clear ( ... )`:\n\
         {cp_body}"
    );

    assert!(
        ups_body.contains("import_routes . clear ("),
        "`finish_upsert_post_commit` must clear `import_routes` on owner source \
         update; got body:\n{ups_body}"
    );
    assert!(
        ups_body.contains("import_routes_known_miss_recorded_at_generation . clear ("),
        "`finish_upsert_post_commit` must clear \
         `import_routes_known_miss_recorded_at_generation` alongside \
         `import_routes`; got body:\n{ups_body}"
    );
}

fn find_fn_body(parsed: &syn::File, target_fn: &str) -> Option<String> {
    for item in &parsed.items {
        if let Some(found) = find_fn_body_in_item(item, target_fn) {
            return Some(found);
        }
    }
    None
}

fn find_fn_body_in_item(item: &syn::Item, target_fn: &str) -> Option<String> {
    use quote::ToTokens;
    match item {
        syn::Item::Fn(f) if f.sig.ident == target_fn => Some(f.block.to_token_stream().to_string()),
        syn::Item::Impl(item_impl) => {
            for impl_item in &item_impl.items {
                if let syn::ImplItem::Fn(f) = impl_item {
                    if f.sig.ident == target_fn {
                        return Some(f.block.to_token_stream().to_string());
                    }
                }
            }
            None
        }
        syn::Item::Mod(m) => {
            if let Some((_, items)) = &m.content {
                for inner in items {
                    if let Some(found) = find_fn_body_in_item(inner, target_fn) {
                        return Some(found);
                    }
                }
            }
            None
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Guard 5: snapshot writer keeps writing both fields.
// ---------------------------------------------------------------------------

/// `set_import_dependencies` is the single producer that admits both
/// `import_routes` (full snapshot) AND
/// `import_routes_known_miss_recorded_at_generation` (computed
/// per-specifier from the caller's known misses). If a future
/// refactor drops either write, the snapshot writer no longer admits
/// known misses correctly and the strict guard above silently passes
/// while the contract has weakened.
#[test]
fn set_import_dependencies_writes_both_route_and_sidecar() {
    let path = workspace_root().join("crates/verter_session/src/host_manage/analysis_io.rs");
    let src =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
    let parsed = syn::parse_file(&src).expect("parse host_manage/analysis_io.rs");
    let body = find_fn_body(&parsed, "set_import_dependencies")
        .expect("`set_import_dependencies` must exist in host_manage/analysis_io.rs");

    // Tokenised body: `value_mut().import_routes = import_routes.clone();`
    // -> `value_mut () . import_routes = import_routes . clone ()`.
    assert!(
        body.contains(". import_routes = import_routes . clone"),
        "`set_import_dependencies` must perform the full-snapshot `import_routes` \
         assignment (`value_mut().import_routes = import_routes.clone()`); \
         body did not contain the assignment:\n{body}"
    );
    assert!(
        body.contains(". import_routes_known_miss_recorded_at_generation = known_miss_generations"),
        "`set_import_dependencies` must admit the known-miss sidecar via \
         `value_mut().import_routes_known_miss_recorded_at_generation = \
         known_miss_generations`; body did not contain the sidecar assignment:\n{body}"
    );
}

// ---------------------------------------------------------------------------
// Sentinel: scanner discriminating-property check.
// ---------------------------------------------------------------------------

/// Inject synthetic fixtures into the scanner and confirm the
/// classification works correctly. This is the discriminating-property
/// proof: the scanner is not vacuous — it FLAGS a sidecar insert from
/// an arbitrary method and an `import_routes.insert` from an
/// arbitrary method, while PASSING the canonical positive helper insert
/// and the lifecycle clears.
#[test]
fn scanner_discriminating_property_fixtures() {
    // Fixture A: arbitrary method inserts into the sidecar — REJECTED.
    let fixture_a = r#"
        impl VerterHost {
            fn arbitrary_writer(&self) {
                let mut derived = self.derived_raw_cache().entry("x".to_string()).or_default();
                derived
                    .value_mut()
                    .import_routes_known_miss_recorded_at_generation
                    .insert("y".to_string(), 7);
            }
        }
    "#;
    assert!(
        !scan_fixture_violations(fixture_a).is_empty(),
        "scanner failed to flag arbitrary-method sidecar insert"
    );

    // Fixture B: arbitrary method inserts into import_routes — REJECTED.
    let fixture_b = r#"
        impl VerterHost {
            fn arbitrary_route_writer(&self) {
                let mut derived = self.derived_raw_cache().entry("x".to_string()).or_default();
                derived
                    .value_mut()
                    .import_routes
                    .insert("y".to_string(), Default::default());
            }
        }
    "#;
    assert!(
        !scan_fixture_violations(fixture_b).is_empty(),
        "scanner failed to flag arbitrary-method import_routes insert"
    );

    // Fixture C: positive helper insert — ACCEPTED.
    let fixture_c = r#"
        impl VerterHost {
            fn cache_positive_import_route_result(&self) {
                let mut derived = self.derived_raw_cache().entry("x".to_string()).or_default();
                derived
                    .value_mut()
                    .import_routes
                    .insert("y".to_string(), Default::default());
            }
        }
    "#;
    assert!(
        scan_fixture_violations(fixture_c).is_empty(),
        "scanner incorrectly flagged the canonical positive helper insert: {:?}",
        scan_fixture_violations(fixture_c)
    );

    // Fixture D: lifecycle clear of both fields — ACCEPTED.
    let fixture_d = r#"
        impl VerterHost {
            fn configure_projects(&self) {
                for mut entry in self.derived_raw_cache().iter_mut() {
                    entry.import_routes.clear();
                    entry.import_routes_known_miss_recorded_at_generation.clear();
                }
            }
        }
    "#;
    assert!(
        scan_fixture_violations(fixture_d).is_empty(),
        "scanner incorrectly flagged lifecycle clear: {:?}",
        scan_fixture_violations(fixture_d)
    );

    // Fixture E: arbitrary method clears import_routes — REJECTED.
    let fixture_e = r#"
        impl VerterHost {
            fn arbitrary_clearer(&self) {
                let mut derived = self.derived_raw_cache().entry("x".to_string()).or_default();
                derived.value_mut().import_routes.clear();
            }
        }
    "#;
    assert!(
        !scan_fixture_violations(fixture_e).is_empty(),
        "scanner failed to flag arbitrary-method import_routes.clear()"
    );

    // Fixture F: snapshot writer assigns both — ACCEPTED.
    let fixture_f = r#"
        impl VerterHost {
            fn set_import_dependencies(&self) {
                let mut derived = self.derived_raw_cache().entry("x".to_string()).or_default();
                derived.value_mut().import_routes = Default::default();
                derived.value_mut().import_routes_known_miss_recorded_at_generation =
                    Default::default();
            }
        }
    "#;
    assert!(
        scan_fixture_violations(fixture_f).is_empty(),
        "scanner incorrectly flagged the snapshot writer assignments: {:?}",
        scan_fixture_violations(fixture_f)
    );

    // Fixture G: pure read — NEVER flagged.
    let fixture_g = r#"
        impl VerterHost {
            fn pure_read(&self) {
                let derived = self.derived_raw_cache().get("x").unwrap();
                let _ = derived.import_routes.len();
                let _ = derived.import_routes_known_miss_recorded_at_generation.iter();
            }
        }
    "#;
    assert!(
        scan_fixture_violations(fixture_g).is_empty(),
        "scanner incorrectly flagged a pure read: {:?}",
        scan_fixture_violations(fixture_g)
    );

    // Fixture H: arbitrary method admits via `.entry(_).or_insert(_)`
    // on `import_routes` — REJECTED. This is the false-negative that
    // motivated tightening the scanner: the outer `.or_insert(_)` has
    // a `MethodCall` receiver (`.entry(_)`), not a field expression,
    // so the direct-shape rule never sees the outer call; the inner
    // `.entry(_)` is what we flag. The mutating-chain rule provides a
    // second flag with a clearer message.
    let fixture_h = r#"
        impl VerterHost {
            fn arbitrary_entry_or_insert(&self) {
                let mut derived = self.derived_raw_cache().entry("x".to_string()).or_default();
                derived
                    .value_mut()
                    .import_routes
                    .entry("y".to_string())
                    .or_insert(Default::default());
            }
        }
    "#;
    assert!(
        !scan_fixture_violations(fixture_h).is_empty(),
        "scanner failed to flag arbitrary-method `import_routes.entry(_).or_insert(_)` chain"
    );

    // Fixture I: arbitrary method admits via
    // `.entry(_).or_default()` on `import_routes` — REJECTED.
    let fixture_i = r#"
        impl VerterHost {
            fn arbitrary_entry_or_default(&self) {
                let mut derived = self.derived_raw_cache().entry("x".to_string()).or_default();
                let _ = derived.value_mut().import_routes.entry("y".to_string()).or_default();
            }
        }
    "#;
    assert!(
        !scan_fixture_violations(fixture_i).is_empty(),
        "scanner failed to flag arbitrary-method `import_routes.entry(_).or_default()` chain"
    );

    // Fixture J: arbitrary method admits via
    // `.entry(_).and_modify(_)` on the sidecar — REJECTED. The
    // strict sidecar has no entry-chain producer; only direct
    // assignment in `set_import_dependencies` is admissible.
    let fixture_j = r#"
        impl VerterHost {
            fn arbitrary_entry_and_modify(&self) {
                let mut derived = self.derived_raw_cache().entry("x".to_string()).or_default();
                derived
                    .value_mut()
                    .import_routes_known_miss_recorded_at_generation
                    .entry("y".to_string())
                    .and_modify(|g| *g = 0);
            }
        }
    "#;
    assert!(
        !scan_fixture_violations(fixture_j).is_empty(),
        "scanner failed to flag arbitrary-method `sidecar.entry(_).and_modify(_)` chain"
    );

    // Fixture K: arbitrary method mutably iterates `import_routes` —
    // REJECTED. `iter_mut()` hands out `&mut V` which permits
    // arbitrary out-of-band mutation; only the admission allow-list
    // may obtain a mutable iterator on the field.
    let fixture_k = r#"
        impl VerterHost {
            fn arbitrary_iter_mut(&self) {
                let mut derived = self.derived_raw_cache().entry("x".to_string()).or_default();
                if let Some((_, v)) = derived.value_mut().import_routes.iter_mut().next() {
                    *v = Default::default();
                }
            }
        }
    "#;
    assert!(
        !scan_fixture_violations(fixture_k).is_empty(),
        "scanner failed to flag arbitrary-method `import_routes.iter_mut()` mutable iteration"
    );

    // Fixture L: canonical positive helper that uses
    // `.entry(_).or_default()` directly on `import_routes` —
    // ACCEPTED. This shape isn't used by the production helper today
    // (it inserts via `.insert(_)` instead) but the scanner must
    // still permit it inside the admission allow-list so a future
    // refactor can use the Entry API without re-opening the false
    // negative.
    let fixture_l = r#"
        impl VerterHost {
            fn cache_positive_import_route_result(&self) {
                let mut derived = self.derived_raw_cache().entry("x".to_string()).or_default();
                let _ = derived.value_mut().import_routes.entry("y".to_string()).or_default();
            }
        }
    "#;
    assert!(
        scan_fixture_violations(fixture_l).is_empty(),
        "scanner incorrectly flagged the canonical positive helper entry-chain: {:?}",
        scan_fixture_violations(fixture_l)
    );

    // Fixture M: snapshot writer mutably iterates `import_routes`
    // (e.g. to retroactively tag entries during admission) — ACCEPTED.
    // `set_import_dependencies` is the snapshot producer and is on
    // the admission allow-list for the positive route map.
    let fixture_m = r#"
        impl VerterHost {
            fn set_import_dependencies(&self) {
                let mut derived = self.derived_raw_cache().entry("x".to_string()).or_default();
                for (_, v) in derived.value_mut().import_routes.iter_mut() {
                    let _ = v;
                }
            }
        }
    "#;
    assert!(
        scan_fixture_violations(fixture_m).is_empty(),
        "scanner incorrectly flagged the snapshot writer iter_mut: {:?}",
        scan_fixture_violations(fixture_m)
    );
}

fn scan_fixture_violations(src: &str) -> Vec<Violation> {
    let parsed = syn::parse_file(src).expect("parse fixture");
    let mut violations = Vec::new();
    let fake_path = Path::new("<fixture>");
    let mut scanner = Scanner::new(fake_path, &mut violations);
    scanner.visit_file(&parsed);
    violations
}
