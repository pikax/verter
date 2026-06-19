//! Discriminating arch-guard: `finalise_signature_or_empty` (the
//! helper that collapsed `FactReadSetFinalise::Overflow` into an
//! empty signature and let the producer publish anyway) has been
//! deleted, and **no production callsite outside the substrate
//! helper `empty_fact_signature()` constructs an empty
//! `Arc<[FactVersionRef]>`** through any covered variant.
//!
//! The guard is an AST walk built on `syn::parse_file` +
//! `syn::visit::Visit`. The walker is structurally type-anchored: a
//! finding fires ONLY when a syntactic signal binds the element type
//! to `FactVersionRef` AT THE BOUNDARY EXPRESSION that produces the
//! suspected empty rail.
//!
//! Boundary expressions checked (each must be both empty-shaped AND
//! type-anchored to FactVersionRef):
//!
//!   * `let x: Arc<[FactVersionRef]> = <RHS>;` — typed `Local`
//!     binding. The RHS is checked recursively for empty-shape.
//!     Covers `Arc::from([])` (type-inferred element),
//!     `Arc::from(&[] as &[FactVersionRef])` (cast),
//!     `Arc::from(vec![].into_boxed_slice())` (boxed-slice empty),
//!     `[].into()`, `vec![].into_boxed_slice().into()` etc., AND the
//!     two-line case where the `:` ascription is on the previous
//!     line from the RHS (syn re-merges the local).
//!   * Function tail expression — when the function's return type
//!     mentions `FactVersionRef`. Covers helper-returned empties:
//!     `fn make_empty() -> Arc<[FactVersionRef]> { Arc::from([]) }`.
//!   * Turbofished call:
//!     - `Arc::<[FactVersionRef]>::from(<empty>)`
//!     - `Arc::from_iter::<FactVersionRef, _>(<anything>)`
//!     - the inner `iter::empty::<FactVersionRef>()` is treated as an
//!       empty constructor when it appears inside one of the above.
//!
//! Empty-construction shapes recognised inside a boundary:
//!
//!   * `[]` (empty array literal)
//!   * `&[]` / `&[] as &[_]`
//!   * `vec![]` macro (empty body)
//!   * `Vec::new()` / `Vec::<_>::new()`
//!   * `iter::empty::<_>()` / `std::iter::empty::<_>()` /
//!     `core::iter::empty::<_>()`
//!   * `vec![].into_boxed_slice()` (empty boxed slice)
//!   * `Arc::from(<empty>)` and `Arc::from_iter(<empty>)` (recursive)
//!   * `<empty>.into()` / `<empty>.into_boxed_slice()` /
//!     `<empty>.collect()` (method chains that wrap the empty
//!     receiver in another wrapper)
//!
//! Allow-listed: `fact_signature_helpers.rs::empty_fact_signature()`
//! is the substrate helper itself. No other production callsite may
//! bypass the helper.
//!
//! Self-validating fixtures below assert the walker matches every
//! known bypass variant and rejects every benign look-alike. Adding
//! a new variant must add a fixture entry.

use std::path::{Path, PathBuf};

use syn::visit::Visit;
use syn::{Expr, ExprCall, ImplItemFn, ItemFn, Local, ReturnType, Type};

/// Walk every `.rs` file under `crates/verter_session/src/` and
/// assert no file references `finalise_signature_or_empty` (deleted)
/// and no file syntactically constructs an empty
/// `Arc<[FactVersionRef]>` outside the substrate helper
/// `fact_signature_helpers::empty_fact_signature()`.
#[test]
fn no_call_site_constructs_empty_signature_from_overflow() {
    let crate_src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut entries: Vec<PathBuf> = Vec::new();
    collect_rs_files(&crate_src, &mut entries);
    assert!(
        !entries.is_empty(),
        "fixture invariant: at least some .rs files MUST be walked"
    );

    let mut finalise_callers: Vec<String> = Vec::new();
    let mut bypass_findings: Vec<BypassFinding> = Vec::new();

    // The sole allowlisted site: the substrate helper itself
    // (`empty_fact_signature` in `fact_signature_helpers.rs`).
    // No other production callsite may bypass the helper.
    let allowed_files = ["fact_signature_helpers.rs"];

    for path in &entries {
        let src = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        let rel = path
            .strip_prefix(&crate_src)
            .unwrap_or(path)
            .display()
            .to_string();

        // Banned identifier: the function name itself. No residual
        // references in production source.
        if src.contains("finalise_signature_or_empty") {
            finalise_callers.push(rel.clone());
        }

        let normalized_rel = rel.replace('\\', "/");
        if allowed_files.iter().any(|a| normalized_rel == *a) {
            continue;
        }

        // Necessary-condition pre-filter: every empty-rail boundary the
        // `BypassWalker` flags is structurally type-anchored to the
        // identifier `FactVersionRef` (the typed `let` binding, the
        // `-> Arc<[FactVersionRef]>` return, the turbofish, and the
        // argument anchor all require it). A file that never mentions
        // `FactVersionRef` cannot produce a single finding, so the `syn`
        // parse + walk is pure overhead there. The banned-identifier
        // half (`finalise_signature_or_empty`) is already handled above
        // by a textual scan, so skipping the AST walk here cannot hide
        // either class of violation.
        if !src.contains("FactVersionRef") {
            continue;
        }

        // The structural arch-guard requires every production file
        // to parse via `syn`. A parse failure indicates either a
        // stale `syn` version or newer Rust syntax the walker has
        // not been updated to recognise — both of which would let a
        // cfg-gated / new-syntax file slip a bypass past the
        // structural check. Fail loudly so the gap is fixed at
        // source rather than masked behind a silent fallback.
        let syntax = syn::parse_file(&src).unwrap_or_else(|e| {
            panic!(
                "structural empty-fact-rail guard requires every production file to parse \
                 via `syn`. `{}` failed: {e}. If this is a legitimate parse-failure (newer \
                 Rust syntax, cfg-gated form `syn` cannot reach), update the walker — do \
                 NOT mask the gap with a silent skip.",
                path.display()
            )
        });

        let mut walker = BypassWalker::new(rel.clone());
        walker.visit_file(&syntax);
        bypass_findings.extend(walker.findings);
    }

    assert!(
        finalise_callers.is_empty(),
        "`finalise_signature_or_empty` MUST be deleted (it collapsed Overflow → \
         empty and let the producer publish anyway). Residual references in: {:?}",
        finalise_callers
    );
    assert!(
        bypass_findings.is_empty(),
        "no production callsite outside `fact_signature_helpers.rs::empty_fact_signature` \
         may construct an empty `Arc<[FactVersionRef]>` through any covered variant. \
         Every such site must route through the substrate helper so the empty rail allocates \
         in exactly one place. Findings: {:#?}",
        bypass_findings
    );
}

/// A flagged bypass: where it was found + a short label of the shape
/// for diagnostics.
#[derive(Debug)]
#[allow(dead_code)]
struct BypassFinding {
    file: String,
    label: &'static str,
    boundary: &'static str,
}

/// AST walker. Checks empty-rail boundaries ONLY at structurally
/// type-anchored sites.
struct BypassWalker {
    file: String,
    findings: Vec<BypassFinding>,
}

impl BypassWalker {
    fn new(file: String) -> Self {
        Self {
            file,
            findings: Vec::new(),
        }
    }

    fn flag(&mut self, label: &'static str, boundary: &'static str) {
        self.findings.push(BypassFinding {
            file: self.file.clone(),
            label,
            boundary,
        });
    }

    /// Boundary: typed `Local`. Check if the binding type IS
    /// `Arc<[FactVersionRef]>` (NOT merely "mentions" FactVersionRef
    /// — `Vec<FactVersionRef>` is an intermediate container, not the
    /// empty rail) AND the RHS is empty-shaped.
    fn check_typed_local(&mut self, local: &Local) {
        let syn::Pat::Type(pt) = &local.pat else {
            return;
        };
        if !type_is_arc_slice_of_fact_version_ref(&pt.ty) {
            return;
        }
        let Some(init) = &local.init else {
            return;
        };
        if let Some(label) = classify_empty_expr(&init.expr) {
            self.flag(label, "typed `let` binding to `Arc<[FactVersionRef]>`");
        }
    }

    /// Boundary: function tail expression. Check if the return type
    /// mentions FactVersionRef AND the tail expr is empty-shaped.
    fn check_fn_tail(&mut self, output: &ReturnType, block: &syn::Block) {
        if !return_type_mentions_fact_version_ref(output) {
            return;
        }
        let Some(tail) = block.stmts.last() else {
            return;
        };
        // The tail statement of a function body, when it produces a
        // value, is a `Stmt::Expr(expr, None)` (no trailing
        // semicolon). A `Stmt::Expr(_, Some(_))` discards the value
        // and is NOT a tail expression.
        if let syn::Stmt::Expr(expr, None) = tail {
            if let Some(label) = classify_empty_expr(expr) {
                self.flag(
                    label,
                    "function tail expression of `-> Arc<[FactVersionRef]>`",
                );
            }
        }
    }

    /// Boundary: turbofished call expression. Check if the callee's
    /// path turbofish mentions FactVersionRef AND the call shape +
    /// arguments are an empty construction.
    fn check_turbofished_call(&mut self, call: &ExprCall) {
        if !call_path_mentions_fact_version_ref(&call.func) {
            return;
        }
        // `Arc::<[FactVersionRef]>::from(<empty>)` and
        // `Arc::from_iter::<FactVersionRef, _>(<empty-or-no-args>)`.
        if let Expr::Path(p) = &*call.func {
            if path_ends_with(&p.path, "from") {
                if call.args.len() == 1 && classify_empty_expr(&call.args[0]).is_some() {
                    self.flag(
                        "`Arc::<[FactVersionRef]>::from(<empty>)`",
                        "FactVersionRef turbofish on `Arc::from`",
                    );
                }
            } else if path_ends_with(&p.path, "from_iter") {
                // `Arc::from_iter::<FactVersionRef, _>(<anything>)` —
                // the turbofish anchors the element type to
                // FactVersionRef; if the inner iterator is empty, it
                // is the empty rail. Empty inner iterator OR no args
                // both qualify.
                if call.args.is_empty()
                    || (call.args.len() == 1 && classify_empty_expr(&call.args[0]).is_some())
                {
                    self.flag(
                        "`Arc::from_iter::<FactVersionRef, _>(<empty>)`",
                        "FactVersionRef turbofish on `Arc::from_iter`",
                    );
                }
            }
        }
    }

    /// Boundary: an `Arc::from(<arg>)` or `Arc::from_iter(<arg>)` call
    /// where the ARGUMENT itself carries the FactVersionRef type
    /// anchor AND classifies as empty.
    ///
    /// This catches the inferred-call shape that escapes the other
    /// boundaries:
    /// ```ignore
    /// fn helper() -> Arc<[FactVersionRef]> {
    ///     let x = Arc::from(Vec::<FactVersionRef>::new());  // no turbofish on Arc::from
    ///     x                                                  // fn tail is just `x`
    /// }
    /// ```
    /// Neither `check_typed_local` (no `: Arc<[FactVersionRef]>`
    /// ascription), nor `check_fn_tail` (the tail expr is `x`, not
    /// the call), nor `check_turbofished_call` (no turbofish on the
    /// callee path) flags this site. But the argument
    /// `Vec::<FactVersionRef>::new()` IS type-anchored on
    /// `FactVersionRef` (its turbofish carries the element type) AND
    /// is an empty constructor — so the resulting `Arc::from(...)` is
    /// the empty-rail allocation regardless of the surrounding
    /// boundary.
    ///
    /// The anchor check is syntactic: does the argument expression
    /// mention the identifier `FactVersionRef` anywhere (turbofish,
    /// slice-cast type, `iter::empty::<FactVersionRef>` qualification,
    /// etc.). The empty-shape check reuses `classify_empty_expr`.
    fn check_arg_anchored_arc_call(&mut self, call: &ExprCall) {
        let Expr::Path(p) = &*call.func else {
            return;
        };
        // Only `Arc::from(...)` and `Arc::from_iter(...)`. Other
        // calls are out of scope for this boundary.
        let is_arc_from = path_is_arc_from(&p.path);
        let is_arc_from_iter = path_is_arc_from_iter(&p.path);
        if !(is_arc_from || is_arc_from_iter) {
            return;
        }
        if call.args.len() != 1 {
            return;
        }
        let arg = &call.args[0];
        // The argument must syntactically anchor the element type to
        // `FactVersionRef`. Identifier search inside the argument
        // expression covers the variants:
        //   * `Vec::<FactVersionRef>::new()`
        //   * `iter::empty::<FactVersionRef>()`
        //   * `&[] as &[FactVersionRef]`
        //   * `Box::<[FactVersionRef]>::new(...)` etc.
        if !expr_mentions_fact_version_ref(arg) {
            return;
        }
        // The argument must classify as an empty construction. A
        // non-empty Vec / non-empty iterator is the live rail, not
        // the empty rail.
        if classify_empty_expr(arg).is_none() {
            return;
        }
        if is_arc_from {
            self.flag(
                "`Arc::from(<FactVersionRef-anchored empty>)` with no callee turbofish",
                "FactVersionRef anchor on argument of `Arc::from`",
            );
        } else {
            self.flag(
                "`Arc::from_iter(<FactVersionRef-anchored empty>)` with no callee turbofish",
                "FactVersionRef anchor on argument of `Arc::from_iter`",
            );
        }
    }
}

impl<'ast> Visit<'ast> for BypassWalker {
    fn visit_item_fn(&mut self, item: &'ast ItemFn) {
        self.check_fn_tail(&item.sig.output, &item.block);
        syn::visit::visit_item_fn(self, item);
    }

    fn visit_impl_item_fn(&mut self, item: &'ast ImplItemFn) {
        self.check_fn_tail(&item.sig.output, &item.block);
        syn::visit::visit_impl_item_fn(self, item);
    }

    fn visit_local(&mut self, local: &'ast Local) {
        self.check_typed_local(local);
        syn::visit::visit_local(self, local);
    }

    fn visit_expr_call(&mut self, call: &'ast ExprCall) {
        self.check_turbofished_call(call);
        self.check_arg_anchored_arc_call(call);
        syn::visit::visit_expr_call(self, call);
    }
}

/// Classify an expression as one of the empty-construction shapes.
/// Returns the shape label if the expression syntactically
/// constructs an empty rail; `None` otherwise.
///
/// This is shape-only (no type anchor) — the caller (a "boundary"
/// check above) is responsible for confirming the surrounding type
/// context anchors the element type to FactVersionRef.
fn classify_empty_expr(expr: &Expr) -> Option<&'static str> {
    match expr {
        // Empty array literal: `[]`.
        Expr::Array(arr) if arr.elems.is_empty() => Some("empty array literal `[]`"),
        // `&[]` or `&[] as &[_]`.
        Expr::Reference(r) => {
            if let Expr::Array(arr) = &*r.expr {
                if arr.elems.is_empty() {
                    return Some("empty reference array `&[]`");
                }
            }
            None
        }
        // Cast: `&[] as &[_]` — the inner empty array escapes via the
        // cast.
        Expr::Cast(cast) => {
            if let Expr::Reference(r) = &*cast.expr {
                if let Expr::Array(arr) = &*r.expr {
                    if arr.elems.is_empty() {
                        return Some("empty slice cast `&[] as &[_]`");
                    }
                }
            }
            None
        }
        // `vec![]` — empty macro invocation. The macro path is `vec`;
        // `tokens` is empty.
        Expr::Macro(m) => {
            let path_is_vec = m
                .mac
                .path
                .segments
                .last()
                .map(|s| s.ident == "vec")
                .unwrap_or(false);
            if path_is_vec && m.mac.tokens.is_empty() {
                return Some("empty `vec![]` macro");
            }
            None
        }
        // `Vec::new()` / `Vec::<_>::new()` / `std::vec::Vec::new()`.
        // Also `iter::empty::<_>()` / `std::iter::empty::<_>()` /
        // `core::iter::empty::<_>()`.
        // Also `Arc::from(<empty>)` / `Arc::from_iter(<empty>)`
        // (recursive).
        Expr::Call(call) => {
            if let Expr::Path(p) = &*call.func {
                if call.args.is_empty() {
                    if path_is_vec_new(&p.path) {
                        return Some("`Vec::new()` constructor");
                    }
                    if path_is_iter_empty(&p.path) {
                        return Some("`iter::empty()` iterator");
                    }
                }
                if path_is_arc_from(&p.path)
                    && call.args.len() == 1
                    && classify_empty_expr(&call.args[0]).is_some()
                {
                    return Some("`Arc::from(<empty>)`");
                }
                if path_is_arc_from_iter(&p.path) {
                    if call.args.is_empty() {
                        return Some("`Arc::from_iter()` with no args");
                    }
                    if call.args.len() == 1 && classify_empty_expr(&call.args[0]).is_some() {
                        return Some("`Arc::from_iter(<empty>)`");
                    }
                }
            }
            None
        }
        // Method-call chain: a wrapper around an empty receiver.
        // `vec![].into_boxed_slice()`, `vec![].into()`,
        // `[].into()`, `Vec::new().into_boxed_slice()`, etc.
        Expr::MethodCall(m) => {
            let method = m.method.to_string();
            if matches!(
                method.as_str(),
                "into" | "into_boxed_slice" | "collect" | "as_slice"
            ) && classify_empty_expr(&m.receiver).is_some()
            {
                return Some("empty `.into()`-chained constructor");
            }
            None
        }
        // Block expression `{ ... <tail> }` — the value is the tail
        // statement. A bare-block-wrapped empty rail is still the
        // same rail.
        Expr::Block(b) => {
            let tail = b.block.stmts.last()?;
            if let syn::Stmt::Expr(inner, None) = tail {
                return classify_empty_expr(inner);
            }
            None
        }
        _ => None,
    }
}

/// True iff a `Type` is structurally `Arc<[FactVersionRef]>` (the
/// empty-rail's actual type) — NOT any type that merely mentions
/// `FactVersionRef`. A `let mut facts: Vec<FactVersionRef> =
/// Vec::new();` intermediate container is not the empty rail and
/// must not be flagged.
///
/// Accepted shapes (recursively unwrapping a single layer of `Arc<>`
/// or `Option<Arc<...>>`):
///   * `Arc<[FactVersionRef]>` — the direct empty-rail shape.
///   * `Option<Arc<[FactVersionRef]>>` — the optional empty-rail.
///   * Any path whose last segment is `Arc` (or a path ending in
///     `Arc`) with a single generic argument that is a slice type
///     whose element is `FactVersionRef`.
fn type_is_arc_slice_of_fact_version_ref(ty: &Type) -> bool {
    match ty {
        Type::Path(p) => {
            // Find the LAST segment — `std::sync::Arc<[X]>` and
            // `Arc<[X]>` both qualify.
            let Some(last) = p.path.segments.last() else {
                return false;
            };
            let ident = last.ident.to_string();
            // `Option<Arc<[FactVersionRef]>>`: recurse on the inner
            // generic argument.
            if ident == "Option" {
                if let syn::PathArguments::AngleBracketed(args) = &last.arguments {
                    for a in &args.args {
                        if let syn::GenericArgument::Type(inner) = a {
                            if type_is_arc_slice_of_fact_version_ref(inner) {
                                return true;
                            }
                        }
                    }
                }
                return false;
            }
            // `Arc<[FactVersionRef]>` — exactly the empty-rail shape.
            if ident == "Arc" {
                if let syn::PathArguments::AngleBracketed(args) = &last.arguments {
                    for a in &args.args {
                        if let syn::GenericArgument::Type(Type::Slice(slice)) = a {
                            if let Type::Path(elem_path) = &*slice.elem {
                                if elem_path
                                    .path
                                    .segments
                                    .last()
                                    .map(|s| s.ident == "FactVersionRef")
                                    .unwrap_or(false)
                                {
                                    return true;
                                }
                            }
                        }
                    }
                }
            }
            false
        }
        _ => false,
    }
}

/// True iff a `ReturnType` is `Arc<[FactVersionRef]>` (or wrappers
/// thereof).
fn return_type_mentions_fact_version_ref(rt: &ReturnType) -> bool {
    match rt {
        ReturnType::Default => false,
        ReturnType::Type(_, ty) => type_is_arc_slice_of_fact_version_ref(ty),
    }
}

/// True iff a call expression's callee path carries a turbofish
/// mentioning `FactVersionRef`. Covers
/// `Arc::<[FactVersionRef]>::from(...)` and
/// `Arc::from_iter::<FactVersionRef, _>(...)`.
fn call_path_mentions_fact_version_ref(callee: &Expr) -> bool {
    if let Expr::Path(p) = callee {
        let mut v = IdentSearch {
            target: "FactVersionRef",
            found: false,
        };
        for seg in &p.path.segments {
            v.visit_path_arguments(&seg.arguments);
            if v.found {
                return true;
            }
        }
    }
    false
}

/// True iff `expr` syntactically mentions the identifier
/// `FactVersionRef` anywhere in its tree (turbofish, slice-cast,
/// `iter::empty::<FactVersionRef>` qualification, type-ascribed
/// reference, etc.). Used to detect element-type anchoring on the
/// ARGUMENT of an `Arc::from(...)` / `Arc::from_iter(...)` call
/// whose callee path itself carries no turbofish.
fn expr_mentions_fact_version_ref(expr: &Expr) -> bool {
    let mut v = IdentSearch {
        target: "FactVersionRef",
        found: false,
    };
    v.visit_expr(expr);
    v.found
}

/// `Vec::new` / `Vec::<_>::new` / `std::vec::Vec::new`.
fn path_is_vec_new(path: &syn::Path) -> bool {
    let segs: Vec<_> = path.segments.iter().map(|s| s.ident.to_string()).collect();
    matches!(segs.last().map(String::as_str), Some("new")) && segs.iter().any(|s| s == "Vec")
}

/// `iter::empty` / `std::iter::empty` / `core::iter::empty`.
fn path_is_iter_empty(path: &syn::Path) -> bool {
    let segs: Vec<_> = path.segments.iter().map(|s| s.ident.to_string()).collect();
    matches!(segs.last().map(String::as_str), Some("empty")) && segs.iter().any(|s| s == "iter")
}

/// `Arc::from` / `Arc::<[_]>::from` / `std::sync::Arc::from` etc.
fn path_is_arc_from(path: &syn::Path) -> bool {
    let segs: Vec<_> = path.segments.iter().map(|s| s.ident.to_string()).collect();
    matches!(segs.last().map(String::as_str), Some("from")) && segs.iter().any(|s| s == "Arc")
}

/// `Arc::from_iter` / `Arc::<[_]>::from_iter` etc.
fn path_is_arc_from_iter(path: &syn::Path) -> bool {
    let segs: Vec<_> = path.segments.iter().map(|s| s.ident.to_string()).collect();
    matches!(segs.last().map(String::as_str), Some("from_iter")) && segs.iter().any(|s| s == "Arc")
}

/// True iff the LAST segment of `path` is `name`.
fn path_ends_with(path: &syn::Path, name: &str) -> bool {
    path.segments
        .last()
        .map(|s| s.ident == name)
        .unwrap_or(false)
}

/// Visitor that searches for an identifier by name anywhere in a
/// syn tree.
struct IdentSearch<'a> {
    target: &'a str,
    found: bool,
}

impl<'ast, 'a> Visit<'ast> for IdentSearch<'a> {
    fn visit_ident(&mut self, ident: &'ast syn::Ident) {
        if ident == self.target {
            self.found = true;
        }
    }
}

/// Self-validating fixture: each KNOWN BYPASS variant the brief
/// enumerated MUST be flagged by the walker. Adding a new variant
/// must add a row here.
#[test]
fn ast_walker_flags_known_bypass_variants() {
    let known_bypasses: &[(&str, &str)] = &[
        // Known empty-fact-rail bypass variants: the 5 syntactic
        // forms + the pre-existing structural variants the AST walker
        // handles + the inferred-call shapes where the FactVersionRef
        // anchor lives on the argument (not the callee turbofish).
        (
            "type-inferred empty: `let x: Arc<[FactVersionRef]> = Arc::from([]);`",
            "use std::sync::Arc;\n\
             struct FactVersionRef;\n\
             fn f() {\n  \
                 let _x: Arc<[FactVersionRef]> = Arc::from([]);\n\
             }\n",
        ),
        (
            "slice-cast empty: `Arc::from(&[] as &[FactVersionRef])`",
            "use std::sync::Arc;\n\
             struct FactVersionRef;\n\
             fn f() {\n  \
                 let _x: Arc<[FactVersionRef]> = Arc::from(&[] as &[FactVersionRef]);\n\
             }\n",
        ),
        (
            "boxed-slice empty wrapped in Arc::from",
            "use std::sync::Arc;\n\
             struct FactVersionRef;\n\
             fn f() {\n  \
                 let _x: Arc<[FactVersionRef]> = Arc::from(vec![].into_boxed_slice());\n\
             }\n",
        ),
        (
            "two-line typed local: ascription on the previous line from the RHS",
            "use std::sync::Arc;\n\
             struct FactVersionRef;\n\
             fn f() {\n  \
                 let _x: Arc<[FactVersionRef]>\n      \
                     = Arc::from([]);\n\
             }\n",
        ),
        (
            "helper-returned empty: `fn make_empty() -> Arc<[FactVersionRef]> { Arc::from([]) }`",
            "use std::sync::Arc;\n\
             struct FactVersionRef;\n\
             fn make_empty() -> Arc<[FactVersionRef]> { Arc::from([]) }\n",
        ),
        // Additional variants the AST walker must also catch.
        (
            "`Arc::from(Vec::<FactVersionRef>::new())`",
            "use std::sync::Arc;\n\
             struct FactVersionRef;\n\
             fn f() {\n  \
                 let _x: Arc<[FactVersionRef]> = Arc::from(Vec::<FactVersionRef>::new());\n\
             }\n",
        ),
        (
            "`Arc::from(Vec::<FactVersionRef>::new().into_boxed_slice())`",
            "use std::sync::Arc;\n\
             struct FactVersionRef;\n\
             fn f() {\n  \
                 let _x: Arc<[FactVersionRef]> = Arc::from(Vec::<FactVersionRef>::new().into_boxed_slice());\n\
             }\n",
        ),
        (
            "`Arc::<[FactVersionRef]>::from(Vec::<FactVersionRef>::new())`",
            "use std::sync::Arc;\n\
             struct FactVersionRef;\n\
             fn f() {\n  \
                 let _x = Arc::<[FactVersionRef]>::from(Vec::<FactVersionRef>::new());\n\
             }\n",
        ),
        (
            "`Arc::<[FactVersionRef]>::from([])`",
            "use std::sync::Arc;\n\
             struct FactVersionRef;\n\
             fn f() {\n  \
                 let _x = Arc::<[FactVersionRef]>::from([]);\n\
             }\n",
        ),
        (
            "`Arc::from_iter(std::iter::empty::<FactVersionRef>())`",
            "use std::sync::Arc;\n\
             struct FactVersionRef;\n\
             fn f() {\n  \
                 let _x: Arc<[FactVersionRef]> = Arc::from_iter(std::iter::empty::<FactVersionRef>());\n\
             }\n",
        ),
        (
            "`Arc::from_iter::<FactVersionRef, _>([])`",
            "use std::sync::Arc;\n\
             struct FactVersionRef;\n\
             fn f() {\n  \
                 let _x = Arc::from_iter::<FactVersionRef, _>([]);\n\
             }\n",
        ),
        (
            "`vec![].into_boxed_slice().into()` with FactVersionRef ascription",
            "use std::sync::Arc;\n\
             struct FactVersionRef;\n\
             fn f() {\n  \
                 let _x: Arc<[FactVersionRef]> = vec![].into_boxed_slice().into();\n\
             }\n",
        ),
        (
            "`[].into()` with FactVersionRef ascription",
            "use std::sync::Arc;\n\
             struct FactVersionRef;\n\
             fn f() {\n  \
                 let _x: Arc<[FactVersionRef]> = [].into();\n\
             }\n",
        ),
        // Inferred-call shapes: the FactVersionRef anchor lives on
        // the ARGUMENT of `Arc::from(...)` / `Arc::from_iter(...)`,
        // not on the callee turbofish or the surrounding `let` /
        // return type. The previous walker checked only callee
        // turbofish, typed-local, and fn-tail boundaries — leaving
        // these forms unflagged when the typed binding was elided
        // and the fn-tail returned a re-bound identifier instead of
        // the call expression directly.
        (
            "inferred-call: `let x = Arc::from(Vec::<FactVersionRef>::new()); x`",
            "use std::sync::Arc;\n\
             struct FactVersionRef;\n\
             fn helper() -> Arc<[FactVersionRef]> {\n  \
                 let x = Arc::from(Vec::<FactVersionRef>::new());\n  \
                 x\n\
             }\n",
        ),
        (
            "inferred-call: `let x = Arc::from_iter(iter::empty::<FactVersionRef>()); x`",
            "use std::sync::Arc;\n\
             use std::iter;\n\
             struct FactVersionRef;\n\
             fn helper() -> Arc<[FactVersionRef]> {\n  \
                 let x = Arc::from_iter(iter::empty::<FactVersionRef>());\n  \
                 x\n\
             }\n",
        ),
    ];

    for (label, src) in known_bypasses {
        let parsed = syn::parse_file(src)
            .unwrap_or_else(|e| panic!("self-validating fixture for `{label}` parses: {e}"));
        let mut walker = BypassWalker::new(format!("<fixture: {label}>"));
        walker.visit_file(&parsed);
        assert!(
            !walker.findings.is_empty(),
            "the AST walker MUST flag bypass variant `{label}`. The fixture source is:\n{src}\n\
             A walker that misses this variant lets a future producer bypass \
             `empty_fact_signature()` through that syntactic path."
        );
    }
}

/// Self-validating fixture: the walker MUST NOT false-positive on
/// look-alike but benign expressions. These cover every false-positive
/// surface plus a couple of additional shapes.
#[test]
fn ast_walker_does_not_false_positive_on_benign_shapes() {
    let benign: &[(&str, &str)] = &[
        (
            "non-empty FactVersionRef vec literal — the live signature construction path",
            "use std::sync::Arc;\n\
             #[allow(dead_code)]\n\
             struct FactVersionRef { c: u8, h: u8 }\n\
             fn f() {\n  \
                 let _x: Arc<[FactVersionRef]> = Arc::from(vec![FactVersionRef { c: 1, h: 2 }]);\n\
             }\n",
        ),
        (
            "`Vec::<String>::new()` for a different type — not the empty FactVersionRef rail",
            "use std::sync::Arc;\n\
             fn f() {\n  \
                 let _x: Arc<[String]> = Arc::from(Vec::<String>::new());\n\
             }\n",
        ),
        (
            "`iter::empty::<String>()` for a different type",
            "use std::sync::Arc;\n\
             fn f() {\n  \
                 let _x: Arc<[String]> = Arc::from_iter(std::iter::empty::<String>());\n\
             }\n",
        ),
        (
            "`[].into()` on a `let` without FactVersionRef ascription — unrelated type",
            "use std::sync::Arc;\n\
             fn f() {\n  \
                 let _x: Arc<[String]> = [].into();\n\
             }\n",
        ),
        (
            "`vec![].into_boxed_slice().into()` for an unrelated type",
            "use std::sync::Arc;\n\
             fn f() {\n  \
                 let _x: Arc<[String]> = vec![].into_boxed_slice().into();\n\
             }\n",
        ),
        (
            "`Arc::from_iter::<String, _>(std::iter::empty())` — turbofish type-anchoring check",
            "use std::sync::Arc;\n\
             fn f() {\n  \
                 let _x = Arc::from_iter::<String, _>(std::iter::empty::<String>());\n\
             }\n",
        ),
        (
            "comment-only mention of FactVersionRef in an unrelated function",
            "use std::sync::Arc;\n\
             // see FactVersionRef for the rail shape\n\
             fn f() {\n  \
                 let _x: Arc<[String]> = Arc::from([]);\n\
             }\n",
        ),
        (
            "intermediate `Vec::new()` inside a function returning Arc<[FactVersionRef]> — \
             must NOT flag the intermediate, only the tail expression",
            "use std::sync::Arc;\n\
             struct FactVersionRef;\n\
             fn make() -> Arc<[FactVersionRef]> {\n  \
                 let _scratch: Vec<u8> = Vec::new();\n  \
                 Arc::from([FactVersionRef])\n\
             }\n",
        ),
        (
            // Mirror of the inferred-call positive fixtures with a
            // non-`FactVersionRef` element type. The argument
            // anchor check must reject this: the argument turbofish
            // anchors `String`, not `FactVersionRef`, so this is the
            // unrelated-rail benign case.
            "inferred-call with unrelated element type: \
             `let x = Arc::from(Vec::<String>::new()); x`",
            "use std::sync::Arc;\n\
             fn helper() -> Arc<[String]> {\n  \
                 let x = Arc::from(Vec::<String>::new());\n  \
                 x\n\
             }\n",
        ),
    ];

    for (label, src) in benign {
        let parsed = syn::parse_file(src)
            .unwrap_or_else(|e| panic!("benign fixture for `{label}` parses: {e}"));
        let mut walker = BypassWalker::new(format!("<benign: {label}>"));
        walker.visit_file(&parsed);
        assert!(
            walker.findings.is_empty(),
            "the AST walker MUST NOT false-positive on `{label}`. The fixture source is:\n{src}\n\
             Findings: {:#?}\n\
             Over-eager matching would force unrelated callsites to route through \
             `empty_fact_signature()`.",
            walker.findings
        );
    }
}

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}
