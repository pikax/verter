//! Zero-production-caller fence for the owner-local node-domain raised-shape
//! readiness primitives.
//!
//! The node-domain raised-shape decision surface — the three classifiers
//! (`node_can_shell_raise`, `node_contains_semantic_miss_legacy_equivalent`,
//! `node_is_expanded_surface_legacy_equivalent`), the raised-shape equality
//! primitive (`raised_shape_eq_nodes`, `raised_shape_eq_node_type_expr`), and the
//! node-bearing expansion facade (`materialize_node_bearing_expansion`,
//! `NodeBearingExpansion`) — is owner-local: the classifiers/equality live in
//! `project_semantic_dispatch::raise` (next to the single private exhaustive
//! `SemanticNodeData -> TypeExpr` traversal `raise_node_to_type_expr_core_impl`
//! they compose over), the facade in `host_manage::component_meta_methods`. There
//! is exactly ONE traversal fn, private to `raise`; the facts are a pure VIEW of
//! its output (the legacy `TypeExpr` predicates / the `TypeExpr`-wrapping
//! `RaisedShapeKey`), so anti-drift is STRUCTURAL — nothing crosses a module
//! boundary, no permit, no seam.
//!
//! These seven primitives realize their facts via materialize-then-predicate
//! (zero-drift NOW, NOT the bottom-up node-domain perf end-state), so they MUST
//! stay dormant — ZERO production callers — until the bottom-up node-domain
//! projection replaces them and the Kind-B sites move off
//! `legacy_semantic_type_expr_bridge`. This fence pins that dormancy.
//!
//! This guard is a dormant, defense-in-depth lexical tripwire. It rejects common
//! direct lexical references to the readiness symbols in the guarded production
//! files, using a shared whole-item `syn::Visit` scanner plus precise subtraction
//! for sanctioned definition spans and documented test-only impl-items. It is not
//! a Rust name resolver, macro-expansion proof, cfg satisfiability engine, or
//! semantic alias analysis. Known residuals include unexpanded macro-generated
//! references, semantic aliases/re-exports, complex cfg behavior beyond the
//! explicit skip policy, and `syn::Verbatim` token forms that `syn` does not
//! structurally interpret. These residuals are bounded and acceptable for the
//! interim dormant fence because production reachability is also constrained by
//! independent compiler/ownership rails, and a later structural-confinement
//! replacement re-establishes structural guards.
//!
//! These are owner-local dormant-readiness guards, NOT seam guards. The raiser's
//! own output is pinned independently by the materialize-parity / raise suite
//! (`raised_shape_tests.rs`); this file only fences the dormancy.

use std::collections::BTreeSet;
use std::path::PathBuf;

use syn::visit::Visit;
use walkdir::WalkDir;

// Reuse the ONE rigorous parsed cfg classifier (EXACT canonical-shape
// recogniser) the output guards already share rather than a cruder
// `mentions-test` token matcher. `cfg_is_exactly_test_or_test_support` returns
// test-gated ONLY for `cfg(test)` / `cfg(any(test, feature = "test-support"))`,
// so a production-satisfiable cfg (`cfg(not(test))`, `cfg(any(test,
// debug_assertions))`, `cfg(all(test, unix))`) is treated as PRODUCTION and is
// SCANNED. Divergent classifiers diverge; the fence guards stay on one
// discriminating detector.
use super::handle_capable_consumer_guards::cfg_is_exactly_test_or_test_support;

fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Normalise an identifier's printed spelling to its bare name by stripping a
/// leading raw-identifier `r#` prefix. `proc_macro2::Ident::to_string` /
/// `syn::Ident::to_string` print a raw identifier (`r#node_can_shell_raise`) WITH
/// the `r#` escape, but the readiness symbol filter compares the bare name
/// (`node_can_shell_raise`). The seven readiness symbols are all non-keyword
/// names, so a raw-spelled reference (`r#node_can_shell_raise(..)`) is the SAME
/// identifier under Rust's alternate lexical spelling — normalising here makes the
/// scanner's "same identifier" claim true for that spelling. Used by BOTH ident
/// recording points (`IdentScan::visit_ident` and the macro/attribute token
/// walker `scan_macro_tokens_for_idents`).
fn normalize_ident(s: String) -> String {
    s.strip_prefix("r#").map(str::to_string).unwrap_or(s)
}

#[allow(
    dead_code,
    reason = "shared file-read helper retained alongside crate_root for the readiness fence's \
              defining-file scan ergonomics"
)]
fn read_rel(rel: &str) -> String {
    let path = crate_root().join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

// ===========================================================================
// Zero-production-caller FENCE for the node-domain readiness primitives.
//
// The seven node-domain readiness primitives — the three classifiers
// (`node_can_shell_raise`, `node_contains_semantic_miss_legacy_equivalent`,
// `node_is_expanded_surface_legacy_equivalent`), the raised-shape equality
// primitive (`raised_shape_eq_nodes`, `raised_shape_eq_node_type_expr`), and the
// node-bearing expansion facade (`materialize_node_bearing_expansion`,
// `NodeBearingExpansion`) — realize their facts via materialize-then-predicate
// (raise the node to a full `TypeExpr` through the shared private
// `raise_node_to_type_expr_core_impl`, then apply the legacy `TypeExpr` predicates
// / compare via a `TypeExpr`-wrapping `RaisedShapeKey`). That is zero-drift NOW but
// is NOT the bottom-up node-domain end-state, so they MUST NOT be wired into a
// production decision path until the bottom-up realization replaces them (tracked
// as debt in docs/arch/parselower-design.md). This guard pins ZERO production
// reference sites for all seven; test-only references are allowed.
//
// scanner_invariant: node_domain_readiness_primitives_have_zero_production_callers
// scanner_justification: this guard is a dormant, defense-in-depth lexical
//   tripwire. It rejects common direct lexical references to the readiness symbols
//   in the guarded production files, using a shared whole-item `syn::Visit`
//   scanner plus precise subtraction for sanctioned definition spans and
//   documented test-only impl-items. Identifier tokens are normalised for the
//   raw-identifier (`r#`) spelling before matching (`r#node_can_shell_raise` is
//   the SAME identifier as the bare name), so a raw-spelled reference is caught.
//   Impl blocks are scanned as whole items (only sanctioned definition spans are
//   subtracted, by SPAN); an inherent readiness impl exempts ONLY the head
//   self-naming occurrence (generic args are still scanned). It is not a Rust name
//   resolver, macro-expansion proof, cfg satisfiability engine, or semantic alias
//   analysis. Known residuals include unexpanded macro-generated references,
//   semantic aliases/re-exports, complex cfg behavior beyond the explicit skip
//   policy, and `syn::Verbatim` token forms that `syn` does not structurally
//   interpret — these are bounded and acceptable for the interim dormant fence
//   because production reachability is also constrained by independent
//   compiler/ownership rails, and a later structural-confinement replacement
//   re-establishes structural guards. These seven node-domain readiness
//   primitives realize their facts via
//   materialize-then-predicate (zero-drift now); they are NOT the bottom-up
//   node-domain end-state and MUST NOT be wired into a production decision path
//   before the bottom-up realization replaces them. Rust cannot express "no
//   production caller yet" (a caller compiles).
// mechanism_ruling: structural-confinement-first — no compiler/structural
//   mechanism expresses a zero-production-caller invariant for a pub(crate) item
//   that is intentionally callable by future node-domain code; a scanner is the
//   only available enforcement. The guard reports lexical `syn`-visible
//   production references to the seven readiness symbols in the guarded source
//   files, after excluding exact test-only items and exact readiness definition
//   sites, with identifier tokens normalised for the raw-identifier (`r#`)
//   spelling before matching. Impl blocks are scanned as whole items; only
//   sanctioned definition spans are subtracted. It is not a Rust name resolver,
//   macro-expansion proof, cfg satisfiability engine, or semantic alias analysis.
//   Known residuals (bounded, acceptable for the interim dormant fence) include
//   unexpanded macro-generated references, semantic aliases/re-exports, complex
//   cfg behavior beyond the explicit skip policy, and `syn::Verbatim` token forms
//   that `syn` does not structurally interpret (the GENERAL family — `Item` /
//   `ImplItem` / `Type` / `Expr::Verbatim` and any other — routed to the no-op
//   `visit_token_stream`, NOT scanned). These residuals are acceptable because
//   production reachability is also constrained by independent compiler/ownership
//   rails, and a later structural-confinement replacement re-establishes
//   structural guards. Default-deny: any non-test `syn`-visible reference outside
//   the sanctioned definition spans fires — INCLUDING a same-file reference at
//   MODULE scope, anywhere in an impl HEADER (trait path / self-type / generics /
//   where-clause), in an item ATTRIBUTE (its `Meta::List` argument token tree),
//   OR inside an impl METHOD BODY. Each `Item::Impl` runs through the SAME
//   whole-item identifier walk as every non-impl item (descending attrs,
//   generics, where-clause, trait path, self_ty, and item bodies / nested items
//   automatically) — NO special-casing of which impl sub-parts to scan. The ONLY
//   spans subtracted are: EXACT-`#[cfg(test)]`-gated items / impl-items; a
//   top-level `fn`/`struct`/`type`/`enum` whose OWN NAME is a readiness symbol
//   (its definition + internal wiring); an impl-item (method/const/type) whose
//   OWN NAME is a readiness symbol (the readiness fn relocated as an associated
//   def — its own subtree); and, for an INHERENT `impl <readiness-type>` (no
//   trait), ONLY the HEAD occurrence of the self_ty — the bare outer type-name
//   ident that names the artifact (the `NodeBearingExpansion` at the head of
//   `impl NodeBearingExpansion<…>`). The self_ty's GENERIC ARGUMENTS / nested
//   types ARE descended, so a readiness symbol used as a generic argument on the
//   self-type (`impl NodeBearingExpansion<NodeBearingExpansion<()>>` — the INNER
//   occurrence) is still REPORTED; only the single artifact-naming head
//   occurrence is exempt. A TRAIT impl with a readiness self-type
//   (`impl Trait for NodeBearingExpansion`) is production wiring and is NOT
//   exempt. Subtraction is BY SPAN, not by symbol globally: a reference appearing
//   in BOTH a sanctioned span AND elsewhere is still reported. ALL THREE scan
//   paths (whole-file, per-item, whole-impl) route through ONE shared identifier
//   visitor, so macro-invocation AND attribute-`Meta::List` token trees are
//   descended for the readiness idents CONSISTENTLY everywhere — no scan path is
//   special. The test/production split is the EXACT cfg classifier
//   `cfg_is_exactly_test_or_test_support` applied to item attrs INCLUDING
//   `Item::Use`, so a production caller under `#[cfg(not(test))]` is scanned, not
//   skipped, and a `#[cfg(test)] use …;` is correctly test-gated.
//   Authority: the codex test-tightening-ladder ruling (Q2 — REPLACE the
//   hand-rolled per-impl-piece scanner with a whole-impl scan + narrow
//   subtraction of sanctioned definition spans) + this manager-finalized fix
//   brief + both codex review OUTs (production use guarded off until the
//   bottom-up realization is the explicit acceptance condition for the
//   materialize-then-predicate realization).
// hardening_rounds: 2
// hardening_history: initial src-walk identifier scan excluding test-gated
//   modules; planted-production-call self-test. r1: scope the exclusion to the
//   definition sites + internal wiring and SCAN the defining files' bodies
//   (closed the whole-file-exclusion hole — a same-file production caller is now
//   reported); reuse the strict `cfg_is_exactly_test_or_test_support` classifier
//   (closed the `cfg(not(test))` skip). r2: scan macro-invocation token trees
//   recursively for the readiness idents (closed the macro-token blind spot —
//   `some_macro!(node_can_shell_raise(...))` is now reported), and route ALL item
//   cfg-gating through `readiness_item_attrs` INCLUDING `Item::Use` (closed the
//   `#[cfg(test)] use …readiness-symbol…;` mis-report). BOUND reached: proc-macro
//   EXPANSION (only literal tokens are scanned) and dataflow aliases are NOT
//   chased — recorded as bounded scanner-debt (docs/arch/raised-shape-guard-debt.md).
//   MECHANISM REPLACEMENT (NOT a 3rd add/broaden round — a replacement + claim
//   narrowing, allowed at the bound per structural-confinement-first): the prior
//   defining-file impl handling hand-reconstructed which impl SUB-PARTS to scan
//   (first per-impl-ITEM, then bolting on a separate impl-HEADER token scan), a
//   mechanism that kept MISSING impl sub-constructs round after round (items,
//   then header type-args, and it would still have missed where-clauses + item
//   attributes). Per the codex test-tightening-ladder Q2 ruling, that per-piece
//   enumeration (the `Item::Impl` branch's per-piece reconstruction plus the
//   helpers `scan_defining_impl_items` + `scan_impl_header_for_idents`) was
//   DELETED and REPLACED with a single whole-impl identifier walk that descends
//   every part of the impl (attrs, generics, where-clause, trait path, self_ty,
//   bodies, nested items) BY CONSTRUCTION — the SAME walk used for non-impl items
//   — subtracting ONLY the exact sanctioned definition spans (cfg(test)
//   impl-items; readiness-named impl-items; the self_ty of an INHERENT readiness
//   impl). This covers where-clauses + attributes BY CONSTRUCTION (no longer
//   residual debt) and removes the missing-a-sub-part failure class. SIMULTANEOUS
//   claim NARROWING: the over-claim ("restores exactly the pre-per-item header
//   coverage" / a blanket "same-file caller anywhere is reported") was replaced
//   with the honest lexical-scan statement above (a `syn`-visible reference scan
//   minus exact sanctioned spans; not a name resolver or macro-expansion proof).
//   This is a mechanism REPLACEMENT + claim NARROWING, so hardening_rounds stays
//   2 (it is not a broadening add).
//   CORRECTNESS / CONSISTENCY / DISCLOSURE fixes (NOT a broadening round —
//   each makes an EXISTING claim TRUE, so hardening_rounds stays 2): (G1) the
//   `visit_meta_list` attribute-token descent was FACTORED out of the impl-only
//   `ImplScan` into ONE shared identifier visitor used by ALL THREE scan paths
//   (whole-file `readiness_production_idents`, per-item
//   `readiness_production_idents_of_item`, and the whole-impl walk), so an
//   attribute `Meta::List` ident on a NON-impl item
//   (`#[some_attr(node_can_shell_raise)] fn f() {}`) is now descended — closing
//   the consistency gap where the docs' "attribute (`Meta::List`) token trees are
//   scanned recursively" claim held only inside impls. (G2) the inherent
//   readiness-impl exemption was tightened from skipping the WHOLE self_ty subtree
//   to suppressing ONLY the single HEAD self-naming occurrence (the artifact
//   naming itself); the self_ty node is now VISITED and its generic arguments /
//   nested types descended, so a readiness symbol used as a generic argument on
//   the self-type (`impl NodeBearingExpansion<NodeBearingExpansion<()>>`) IS
//   reported — making the "exempt ONLY the self_ty occurrence" claim true.
//   (P2 disclosure) the `syn::Verbatim` no-op residual (a lexical-visibility gap
//   of the SAME family as macro-EXPANSION — `syn` token forms it does not
//   structurally interpret, routed to the no-op `visit_token_stream`) is
//   ENUMERATED in the scanner_justification / mechanism_ruling / debt ledger
//   alongside macro-EXPANSION / semantic-ALIASING / cfg complexity.
//   TERMINAL (codex-locked — the test-tightening ladder is LOCKED at exactly these
//   two cheap correctness fixes + a claim narrowing; NOT a broadening round, so
//   hardening_rounds stays 2): (Fix1) identifier tokens are NORMALISED for the
//   raw-identifier (`r#`) spelling before matching, in BOTH the structured
//   `visit_ident` path and the macro/attribute token walker
//   `scan_macro_tokens_for_idents` (via the shared `normalize_ident` helper) — a
//   raw-spelled production reference (`r#node_can_shell_raise(..)`) is the SAME
//   identifier under Rust's alternate lexical spelling (the seven symbols are all
//   non-keyword names), so normalisation makes the scanner's existing "same
//   identifier" claim TRUE for that spelling. (Fix2) the EXACT-`#[cfg(test)]`
//   impl-ITEM skip in the shared `visit_impl_item` was HOISTED to UNCONDITIONAL —
//   it now applies on EVERY scan path (whole-file, per-item, whole-impl), not only
//   when the whole-impl `impl_subtraction` is active; only the readiness-OWN-NAME
//   impl-item subtraction stays gated on `impl_subtraction.is_some()` — so the
//   documented "test-gated impl-items are skipped" claim is now TRUE on the
//   whole-file path too. Plus the SIMULTANEOUS claim NARROWING above (the honest
//   "dormant defense-in-depth lexical tripwire … not a name resolver / macro-
//   expansion proof / cfg satisfiability engine / semantic alias analysis" wording;
//   the over-confident "a COMPILING reference cannot hide" framing was DROPPED, and
//   the `syn::Verbatim` residual is the GENERAL family, not the 4 enumerated arms).
//   Both fixes make an EXISTING "same identifier" / "test-gated impl-items skipped"
//   claim TRUE rather than broadening the scanner's reach; raw-ident-beyond-the-`r#`
//   normalisation, if any, is part of the disclosed residual. hardening_rounds
//   stays 2. THE FENCE IS TERMINAL-LOCKED — no further rounds on this tripwire
//   until the bottom-up node-domain realization replaces the readiness primitives
//   and this fence is retired with them.
// ===========================================================================

/// The seven node-domain readiness primitives whose production wiring is fenced
/// off: their facts come from materialize-then-predicate (zero-drift now), and
/// they stay unwired until the bottom-up realization replaces that shape. A
/// reference to any of these in non-test production source — outside the
/// sanctioned definition sites (including inside the defining files themselves)
/// — fires the fence.
const NODE_DOMAIN_READINESS_SYMBOLS: &[&str] = &[
    "node_can_shell_raise",
    "node_contains_semantic_miss_legacy_equivalent",
    "node_is_expanded_surface_legacy_equivalent",
    "raised_shape_eq_nodes",
    "raised_shape_eq_node_type_expr",
    "materialize_node_bearing_expansion",
    "NodeBearingExpansion",
];

/// The files that DEFINE the readiness primitives. These are NOT excluded
/// wholesale — the scan covers their bodies TOO and subtracts ONLY the exact
/// sanctioned DEFINITION spans. Every item, INCLUDING every `Item::Impl`, runs
/// through the SAME whole-item identifier walk (no special-casing of which impl
/// sub-parts to scan): the walk descends attrs, generics, where-clauses, the
/// trait path, the self_ty, and item bodies / nested items automatically. The
/// ONLY spans subtracted are: EXACT-`#[cfg(test)]`-gated items / impl-items; a
/// top-level `fn` / `struct` / `type` / `enum` whose OWN NAME is one of the seven
/// (its definition + its own internal wiring, e.g. `raised_shape_eq_nodes`
/// calling `node_can_shell_raise`, or `materialize_node_bearing_expansion`'s
/// `artifact: &NodeBearingExpansion` param); an impl-item (method / assoc-const /
/// assoc-type) whose OWN NAME is one of the seven (a readiness fn relocated as an
/// associated def — its own subtree); and, for an INHERENT `impl <readiness-type>`
/// (NO trait), ONLY the HEAD occurrence of the self_ty (the bare outer type-name
/// ident that names the artifact in its own inherent header — the self_ty's
/// GENERIC ARGUMENTS / nested types are still scanned, so a readiness symbol used
/// as a generic argument on the self-type, e.g.
/// `impl NodeBearingExpansion<NodeBearingExpansion<()>>`, IS reported). A TRAIT
/// impl whose self-type is a readiness type
/// (`impl Trait for NodeBearingExpansion`) is production wiring and is NOT exempt.
/// Subtraction is BY SPAN, not by symbol globally — a reference appearing in BOTH
/// a sanctioned span AND elsewhere is still reported. A production CALL /
/// REFERENCE of any of the seven that sits OUTSIDE those sanctioned spans —
/// including inside these very files, at module scope, anywhere in a
/// NON-sanctioned impl HEADER (a readiness type as a trait/type argument /
/// where-clause bound / attribute, e.g.
/// `impl Marker<NodeBearingExpansion> for Holder<u8>`), OR inside an impl method
/// body — is REPORTED. (Crate-relative, forward-slashed.) The classifiers +
/// equality are owner-local in `raise.rs`; the facade in
/// `component_meta_methods.rs`.
const READINESS_DEFINING_FILES: &[&str] = &[
    "src/project_semantic_dispatch/raise.rs",
    "src/host_manage/component_meta_methods.rs",
];

/// Is a crate-relative path a TEST file (excluded from the production scan)?
fn is_readiness_test_file(rel: &str) -> bool {
    rel.ends_with("_tests.rs") || rel.ends_with("/tests.rs") || rel.contains("/tests/")
}

/// Does an item carry a cfg that gates it out of EVERY non-test build —
/// i.e. is it EXACTLY test/test-support-gated (`#[cfg(test)]` or
/// `#[cfg(any(test, feature = "test-support"))]`)? Reuses the shared EXACT
/// recogniser [`cfg_is_exactly_test_or_test_support`] rather than a shallow
/// `mentions-test` token scan, so a production-SATISFIABLE cfg is NOT treated as
/// test-only: `#[cfg(not(test))]`, `#[cfg(any(test, debug_assertions))]`, and
/// `#[cfg(all(test, unix))]` all classify as PRODUCTION here and are therefore
/// SCANNED. A production caller hidden under `#[cfg(not(test))]` no longer evades
/// the fence (the shallow matcher's hole — it saw the bare `test` ident inside
/// `not(test)` and wrongly skipped the item).
fn readiness_attrs_are_cfg_test(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|a| {
        if !a.path().is_ident("cfg") {
            return false;
        }
        matches!(&a.meta, syn::Meta::List(list)
            if cfg_is_exactly_test_or_test_support(list.tokens.clone()))
    })
}

/// The SINGLE shared whole-identifier scan visitor used by ALL THREE readiness
/// scan paths — the whole-file scan ([`readiness_production_idents`]), the
/// per-item scan ([`readiness_production_idents_of_item`]), and the whole-impl
/// scan ([`readiness_impl_block_violations`]). Factoring it into ONE type makes
/// the three lexical-visibility overrides — `visit_ident` (record the token),
/// `visit_macro` (descend a macro-invocation's opaque token tree), and
/// `visit_meta_list` (descend an attribute argument list's token tree) — apply
/// CONSISTENTLY everywhere, so no scan path is special: an attribute meta-list
/// (`#[some_attr(NodeBearingExpansion)]`) and a macro-invocation token tree are
/// descended for the readiness idents whether the host item is inside an impl or
/// not. The whole-identifier scan never splits an ident into substrings, so a
/// symbol is reported only when it appears AS a token; string-literal text is a
/// `Literal`, never an `Ident`, so a `reason = "…readiness-symbol…"` is not a hit.
///
/// The impl path additionally supplies `impl_subtraction` so the SAME visitor
/// also subtracts the impl-specific sanctioned spans (an impl-item whose OWN NAME
/// is a readiness symbol; the HEAD occurrence of an INHERENT readiness impl's
/// self_ty) by SPAN — see [`ImplSubtraction`]. For the non-impl paths it is
/// `None`, so only the universal cfg(test) skip applies.
struct IdentScan {
    idents: BTreeSet<String>,
    /// `Some` only for the whole-impl scan: the impl-specific sanctioned-span
    /// subtractions. `None` for the whole-file / per-item scans.
    impl_subtraction: Option<ImplSubtraction>,
}

/// The impl-specific sanctioned-span subtractions applied BY SPAN inside the
/// shared [`IdentScan`] when it walks an `impl` block. (Universal cfg(test) /
/// readiness-named-DEFINITION subtractions are handled outside this struct; this
/// carries only the two impl-local ones.)
struct ImplSubtraction {
    /// For an INHERENT `impl <readiness-type>` (no trait): the head self-type
    /// name to suppress EXACTLY ONCE — the bare outer type-name ident that names
    /// the artifact (the `NodeBearingExpansion` at the head of
    /// `impl NodeBearingExpansion<…>`). `None` for a trait impl or a non-readiness
    /// inherent impl. Set to `Some(name)` while walking the self_ty; the FIRST
    /// head-name occurrence is dropped, every other occurrence (incl. a nested
    /// generic-argument reference of the same name) is still recorded.
    suppress_self_ty_head_name: Option<String>,
    /// True while the visitor is descending the self_ty node of an inherent
    /// readiness impl, so `visit_ident` knows to consider the head-name
    /// suppression. The suppression fires at most once (`head_suppressed`).
    in_self_ty: bool,
    /// Latches once the single sanctioned head-name occurrence has been dropped,
    /// so a later same-name occurrence inside the self_ty's generic arguments is
    /// recorded normally.
    head_suppressed: bool,
}

impl<'ast> Visit<'ast> for IdentScan {
    // Skip whole `#[cfg(test)]` items / modules / impls / fns / USE imports.
    fn visit_item(&mut self, item: &'ast syn::Item) {
        let attrs: &[syn::Attribute] = readiness_item_attrs(item);
        if readiness_attrs_are_cfg_test(attrs) {
            return; // test-gated subtree — excluded
        }
        syn::visit::visit_item(self, item);
    }
    fn visit_impl_item_fn(&mut self, f: &'ast syn::ImplItemFn) {
        if readiness_attrs_are_cfg_test(&f.attrs) {
            return;
        }
        syn::visit::visit_impl_item_fn(self, f);
    }
    // Impl-item subtractions. The EXACT-`#[cfg(test)]` impl-item skip is
    // UNCONDITIONAL — it runs on EVERY scan path (whole-file, per-item,
    // whole-impl), so a test-gated associated const / type / macro inside an impl
    // is NOT descended even on the whole-file scan that carries no
    // `impl_subtraction` (otherwise the documented "test-gated impl-items are
    // skipped" claim would hold only on the whole-impl path). Only the
    // readiness-OWN-NAME subtraction (the readiness def's own associated subtree)
    // stays gated on the whole-impl scan (`impl_subtraction.is_some()`); on the
    // non-impl paths that own-name subtree is descended normally. Everything else
    // is descended normally.
    fn visit_impl_item(&mut self, item: &'ast syn::ImplItem) {
        let (attrs, own_name): (&[syn::Attribute], Option<String>) = match item {
            syn::ImplItem::Fn(f) => (&f.attrs, Some(f.sig.ident.to_string())),
            syn::ImplItem::Const(c) => (&c.attrs, Some(c.ident.to_string())),
            syn::ImplItem::Type(t) => (&t.attrs, Some(t.ident.to_string())),
            syn::ImplItem::Macro(m) => (&m.attrs, None),
            _ => (&[], None),
        };
        // UNCONDITIONAL cfg(test) impl-item skip — applies on all scan paths.
        if readiness_attrs_are_cfg_test(attrs) {
            return;
        }
        // Readiness-own-name subtraction — only on the whole-impl scan.
        if self.impl_subtraction.is_some()
            && own_name
                .as_deref()
                .is_some_and(|s| NODE_DOMAIN_READINESS_SYMBOLS.contains(&s))
        {
            return;
        }
        syn::visit::visit_impl_item(self, item);
    }
    // Record every identifier token anywhere in the (non-test) subtree — EXCEPT
    // the single sanctioned HEAD occurrence of an inherent readiness impl's
    // self_ty (the artifact naming itself). The head suppression fires at most
    // once and ONLY while descending that self_ty; a same-name reference nested
    // in the self_ty's generic arguments (`impl Foo<Foo<()>>`) is still recorded.
    fn visit_ident(&mut self, id: &'ast proc_macro2::Ident) {
        // Normalise a raw-identifier spelling (`r#name` -> `name`) before any
        // matching: a raw-spelled reference is the SAME identifier under Rust's
        // alternate lexical spelling, and the seven readiness symbols are all
        // non-keyword names, so this makes the scanner see the bare name for both
        // the head-suppression comparison and the recorded token set.
        let name = normalize_ident(id.to_string());
        if let Some(sub) = self.impl_subtraction.as_mut() {
            if sub.in_self_ty && !sub.head_suppressed {
                if let Some(head) = sub.suppress_self_ty_head_name.as_deref() {
                    if name == head {
                        sub.head_suppressed = true;
                        return; // drop EXACTLY the head self-naming occurrence
                    }
                }
            }
        }
        self.idents.insert(name);
    }
    // Scan macro-invocation token trees too: `syn`'s default walk does NOT
    // descend into a macro's opaque token stream, so a readiness ident inside
    // `some_macro!(... node_can_shell_raise(...) ...)` would otherwise evade
    // the ident scan. Recurse the token tree, recording every `Ident` token
    // (literals — incl. string-embedded text — are `Literal` tokens, never
    // `Ident`, so they are not hit).
    fn visit_macro(&mut self, m: &'ast syn::Macro) {
        scan_macro_tokens_for_idents(&m.tokens, &mut self.idents);
        syn::visit::visit_macro(self, m);
    }
    // Descend an attribute's `Meta::List` token tree
    // (`#[some_attr(NodeBearingExpansion)]`): the default walk routes the tokens
    // to `visit_token_stream`, which is a NO-OP, so a readiness ident inside an
    // attribute argument list would otherwise evade the scan. Scan its tokens
    // `Ident`-typed (string-literal text inside an attr is a `Literal`, never an
    // `Ident`, so a `reason = "…readiness-symbol…"` is not a hit). The meta PATH
    // (`some_attr`) is still visited by the default walk. Routed through the
    // SHARED visitor so attribute meta-lists are descended CONSISTENTLY on every
    // scan path (whole-file, per-item, and whole-impl) — not only inside impls.
    fn visit_meta_list(&mut self, m: &'ast syn::MetaList) {
        scan_macro_tokens_for_idents(&m.tokens, &mut self.idents);
        syn::visit::visit_meta_list(self, m);
    }
}

impl IdentScan {
    /// A scan with no impl-specific subtraction (the whole-file / per-item paths).
    fn new() -> Self {
        IdentScan {
            idents: BTreeSet::new(),
            impl_subtraction: None,
        }
    }
}

/// Collect every WHOLE-identifier token referenced in NON-test items of a
/// `syn::File` (path segments, method-call names, field names, struct/enum field
/// types, fn signatures, bodies, MACRO-invocation token trees, and ATTRIBUTE
/// meta-list token trees). Items (or enclosing modules / impls / fns) carrying
/// `#[cfg(test)]` are skipped wholesale — their references are test code. Routes
/// through the SHARED [`IdentScan`] visitor, so an attribute meta-list ident
/// (`#[some_attr(node_can_shell_raise)] fn f() {}`) is descended here EXACTLY as
/// it is inside the whole-impl scan. The whole-identifier scan never splits an
/// ident into substrings, so a symbol is reported only when it appears AS a token.
fn readiness_production_idents(file: &syn::File) -> BTreeSet<String> {
    let mut scan = IdentScan::new();
    scan.visit_file(file);
    scan.idents
}

/// The cfg-bearing attribute slice for an item, INCLUDING `syn::Item::Use` (so a
/// `#[cfg(test)] use …;` is correctly test-gated). The earlier classifier did NOT
/// inspect `Item::Use` attrs, so a `#[cfg(test)] use …node_can_shell_raise…;` was
/// wrongly scanned as production (R3-10).
fn readiness_item_attrs(item: &syn::Item) -> &[syn::Attribute] {
    match item {
        syn::Item::Mod(m) => &m.attrs,
        syn::Item::Impl(i) => &i.attrs,
        syn::Item::Fn(f) => &f.attrs,
        syn::Item::Struct(s) => &s.attrs,
        syn::Item::Enum(e) => &e.attrs,
        syn::Item::Const(c) => &c.attrs,
        syn::Item::Static(s) => &s.attrs,
        syn::Item::Type(t) => &t.attrs,
        syn::Item::Use(u) => &u.attrs,
        _ => &[],
    }
}

/// Recursively scan a macro-invocation token stream, inserting every `Ident`
/// token into `out` (NORMALISED via [`normalize_ident`], so a raw-spelled
/// `r#node_can_shell_raise` token records the bare name — the SAME normalisation
/// the structured `visit_ident` path applies). Mirrors the recursive `visit_macro`
/// token-tree scan the `output_projector_residual_guards` bridge scanner uses:
/// descend each `Group` into its inner stream; ignore non-`Ident` tokens (a
/// raw/byte/char/string literal is a `Literal` token, never an `Ident`, so symbol
/// text inside a string is NOT a hit).
fn scan_macro_tokens_for_idents(tokens: &proc_macro2::TokenStream, out: &mut BTreeSet<String>) {
    for tt in tokens.clone() {
        match tt {
            proc_macro2::TokenTree::Ident(id) => {
                out.insert(normalize_ident(id.to_string()));
            }
            proc_macro2::TokenTree::Group(g) => scan_macro_tokens_for_idents(&g.stream(), out),
            _ => {}
        }
    }
}

/// Which of the seven readiness symbols appear as identifiers in non-test items
/// of `src`? Factored so the discrimination self-test can feed synthetic
/// production source.
fn readiness_symbols_referenced_in(src: &str) -> BTreeSet<String> {
    let Ok(file) = syn::parse_file(src) else {
        return BTreeSet::new();
    };
    let idents = readiness_production_idents(&file);
    NODE_DOMAIN_READINESS_SYMBOLS
        .iter()
        .filter(|sym| idents.contains(**sym))
        .map(|sym| (*sym).to_string())
        .collect()
}

/// Is `item` a SANCTIONED top-level definition site of one of the seven
/// readiness symbols — a `fn` / `struct` / `type` / `enum` whose name is one of
/// the seven? References INSIDE such a site are the definition + its own
/// internal wiring, so the site is skipped wholesale by the defining-file scan;
/// everything else is scanned for production references.
///
/// An `impl` block is NOT classified here: it is run through the SAME whole-item
/// walk used for non-impl items (in [`readiness_impl_block_violations`]), which
/// descends attrs / generics / where-clause / trait path / self_ty / bodies and
/// subtracts ONLY the exact sanctioned definition SPANS (cfg(test) impl-items;
/// impl-items whose own name is a readiness symbol; the self_ty of an INHERENT
/// readiness impl). So a production caller inside an impl method (e.g.
/// `impl NodeBearingExpansion { fn leaks() { node_can_shell_raise(..) } }`) and a
/// readiness type named anywhere in a NON-sanctioned impl header (trait path /
/// type argument / where-clause / attribute, e.g.
/// `impl Marker<NodeBearingExpansion> for Holder<u8>`) are both REPORTED.
fn readiness_item_is_sanctioned_definition(item: &syn::Item) -> bool {
    let names = |s: &str| NODE_DOMAIN_READINESS_SYMBOLS.contains(&s);
    match item {
        syn::Item::Fn(f) => names(&f.sig.ident.to_string()),
        syn::Item::Struct(s) => names(&s.ident.to_string()),
        syn::Item::Type(t) => names(&t.ident.to_string()),
        syn::Item::Enum(e) => names(&e.ident.to_string()),
        _ => false,
    }
}

/// Last path-segment ident of an impl self-type, unwrapping references / groups
/// / parens (mirrors the output-guard carrier classifier; kept local so this
/// module has no cross-guard dependency for the small helper). Used to decide
/// whether an INHERENT `impl <readiness-type>` block names the artifact as its
/// OWN self-type — in which case the whole-impl walk subtracts ONLY the single
/// HEAD occurrence of that self-type name (the artifact naming itself in its own
/// inherent header), still descending the rest of the impl AND the self_ty's own
/// generic arguments / nested types (so an inner readiness reference fires).
fn impl_self_ty_last_ident_local(ty: &syn::Type) -> Option<String> {
    match ty {
        syn::Type::Path(p) => p.path.segments.last().map(|s| s.ident.to_string()),
        syn::Type::Reference(r) => impl_self_ty_last_ident_local(&r.elem),
        syn::Type::Group(g) => impl_self_ty_last_ident_local(&g.elem),
        syn::Type::Paren(p) => impl_self_ty_last_ident_local(&p.elem),
        _ => None,
    }
}

/// Scan a DEFINING file's body for production references to the seven readiness
/// symbols that sit OUTSIDE the sanctioned definition spans. Walks the file's
/// items: a `#[cfg(test)]`-EXACT-gated item is skipped (test code); a sanctioned
/// top-level definition (a `fn` / `struct` / `type` / `enum` named for one of
/// the seven) is skipped wholesale (definition + internal wiring); a `mod`
/// recurses; an `impl` block runs through the WHOLE-impl identifier walk
/// ([`readiness_impl_block_violations`]) — the SAME walk used for non-impl items,
/// descending attrs / generics / where-clause / trait path / self_ty / bodies —
/// subtracting ONLY the exact sanctioned spans (cfg(test) impl-items;
/// readiness-named impl-items; the self_ty of an INHERENT readiness impl); ANY
/// OTHER item is scanned for the seven names and each hit is reported. So a
/// production reference of any of the seven INSIDE a defining file — at module
/// scope, anywhere in a non-sanctioned impl HEADER (trait/type argument,
/// where-clause bound, or attribute), OR inside an impl method body — is
/// REPORTED.
fn readiness_defining_file_violations(src: &str) -> BTreeSet<String> {
    let Ok(file) = syn::parse_file(src) else {
        return BTreeSet::new();
    };
    let mut out = BTreeSet::new();
    scan_defining_items(&file.items, &mut out);
    out
}

/// Recursive item-walker for [`readiness_defining_file_violations`].
fn scan_defining_items(items: &[syn::Item], out: &mut BTreeSet<String>) {
    for item in items {
        // Skip EXACT-test-gated subtrees (their references are test code).
        // `readiness_item_attrs` includes `Item::Use`, so a `#[cfg(test)] use …;`
        // is correctly test-gated.
        if readiness_attrs_are_cfg_test(readiness_item_attrs(item)) {
            continue;
        }
        // A sanctioned TOP-LEVEL definition (fn/struct/type/enum named for one of
        // the seven) is skipped wholesale (definition + internal wiring).
        if readiness_item_is_sanctioned_definition(item) {
            continue;
        }
        // A non-test, non-definition module recurses (a production caller could
        // hide inside an inline `mod`).
        if let syn::Item::Mod(m) = item {
            if let Some((_, inner)) = &m.content {
                scan_defining_items(inner, out);
            }
            continue;
        }
        // An `impl` block is NOT skipped wholesale and is NOT scanned
        // piece-by-piece. It runs through the SAME whole-item identifier walk used
        // for every non-impl item (descending attrs, generics, where-clause, trait
        // path, self_ty, item bodies, nested items by construction), subtracting
        // ONLY the exact sanctioned definition spans: EXACT-`#[cfg(test)]`-gated
        // impl-items; impl-items whose OWN NAME is a readiness symbol; and, for an
        // INHERENT `impl <readiness-type>` (no trait), ONLY the HEAD occurrence of
        // the self_ty (its generic arguments / nested types are still scanned). A
        // TRAIT impl with a readiness self-type is production wiring and is NOT
        // exempt. So a caller inside an impl method body, a readiness type named as
        // a trait/type argument, in a where-clause bound, in an item attribute, or
        // as a generic argument on an inherent readiness self-type is REPORTED.
        if let syn::Item::Impl(i) = item {
            for sym in readiness_impl_block_violations(i) {
                out.insert(sym);
            }
            continue;
        }
        // ANY OTHER item: scan its identifier tokens (skipping nested
        // EXACT-test-gated fns) and report every readiness symbol referenced.
        let idents = readiness_production_idents_of_item(item);
        for sym in NODE_DOMAIN_READINESS_SYMBOLS {
            if idents.contains(*sym) {
                out.insert((*sym).to_string());
            }
        }
    }
}

/// Report the readiness symbols an `impl` block references in PRODUCTION,
/// scanning the WHOLE impl as a single item — the SAME identifier walk used for
/// every non-impl item — and subtracting ONLY the exact sanctioned definition
/// spans. No special-casing of which impl sub-parts to scan: the walk descends
/// attrs, generics, where-clause, trait path, self_ty, item bodies, and nested
/// items automatically, so a readiness reference ANYWHERE in the impl (header
/// type-argument, where-clause bound, attribute, or method body) is caught BY
/// CONSTRUCTION.
///
/// The walk subtracts exactly three sanctioned-span classes — by SPAN (it does
/// not descend them), never by symbol globally:
/// - an EXACT-`#[cfg(test)]`-gated impl-item / nested item / impl-fn (test code);
/// - an impl-item (method / assoc-const / assoc-type) whose OWN NAME is a
///   readiness symbol (a readiness fn relocated as an associated def — its own
///   subtree, matching the module-scope sanctioned-definition behaviour: a
///   reference inside the readiness def's own body is its internal wiring, not a
///   caller);
/// - for an INHERENT `impl <readiness-type>` (NO trait), ONLY the HEAD occurrence
///   of the self_ty — the bare outer type-name ident that names the artifact (the
///   `NodeBearingExpansion` at the head of `impl NodeBearingExpansion<…>`). The
///   self_ty node IS visited and its GENERIC ARGUMENTS / nested types ARE
///   descended, so a readiness symbol used as a generic argument on the
///   self-type (`impl NodeBearingExpansion<NodeBearingExpansion<()>>`) — the
///   INNER occurrence — is still REPORTED; only the single artifact-naming head
///   occurrence is exempt. A TRAIT impl whose self-type is a readiness type
///   (`impl Trait for NodeBearingExpansion`) is production wiring and is NOT
///   exempt — its self_ty IS descended with no head suppression.
///
/// Because subtraction is by span, a readiness reference that appears BOTH in a
/// sanctioned span AND elsewhere in the impl is still reported (the elsewhere
/// occurrence is descended normally).
fn readiness_impl_block_violations(i: &syn::ItemImpl) -> BTreeSet<String> {
    // For an INHERENT `impl <readiness-type>` (no trait) the HEAD of the self_ty
    // names the artifact in its own inherent header — subtract ONLY that one head
    // occurrence (G2 precision: the self_ty node is still visited so a readiness
    // symbol nested in its generic arguments / nested types IS reported). A TRAIT
    // impl is NOT exempt — fall through to the plain whole-impl walk that descends
    // self_ty with no head suppression.
    let inherent_readiness_self_ty_head = if i.trait_.is_none() {
        impl_self_ty_last_ident_local(&i.self_ty)
            .filter(|s| NODE_DOMAIN_READINESS_SYMBOLS.contains(&s.as_str()))
    } else {
        None
    };

    let mut scan = IdentScan {
        idents: BTreeSet::new(),
        impl_subtraction: Some(ImplSubtraction {
            suppress_self_ty_head_name: inherent_readiness_self_ty_head.clone(),
            in_self_ty: false,
            head_suppressed: false,
        }),
    };

    if inherent_readiness_self_ty_head.is_some() {
        // Walk every impl part — attrs, generics (incl. where-clause), self_ty,
        // and items — through the SHARED visitor. While descending the self_ty the
        // `in_self_ty` flag is set so the shared `visit_ident` drops EXACTLY the
        // first head-name occurrence; generic arguments / nested types of the
        // self_ty are still recorded (so an inner readiness reference fires).
        for attr in &i.attrs {
            scan.visit_attribute(attr);
        }
        scan.visit_generics(&i.generics);
        if let Some(sub) = scan.impl_subtraction.as_mut() {
            sub.in_self_ty = true;
        }
        scan.visit_type(&i.self_ty);
        if let Some(sub) = scan.impl_subtraction.as_mut() {
            sub.in_self_ty = false;
        }
        for item in &i.items {
            scan.visit_impl_item(item);
        }
    } else {
        scan.visit_item_impl(i);
    }

    NODE_DOMAIN_READINESS_SYMBOLS
        .iter()
        .filter(|sym| scan.idents.contains(**sym))
        .map(|sym| (*sym).to_string())
        .collect()
}

/// Whole-identifier tokens of a SINGLE item, skipping nested `#[cfg(test)]`-EXACT
/// items / impl-fns. Routes through the SHARED [`IdentScan`] visitor, so an
/// attribute meta-list ident (`#[some_attr(node_can_shell_raise)] fn f() {}`) on
/// a NON-impl item is descended here EXACTLY as it is inside the whole-impl scan
/// (the same `visit_ident` / `visit_macro` / `visit_meta_list` discipline).
fn readiness_production_idents_of_item(item: &syn::Item) -> BTreeSet<String> {
    let mut scan = IdentScan::new();
    scan.visit_item(item);
    scan.idents
}

/// Walk `crates/verter_session/src/**`, skipping test files, and return
/// `(rel_path, symbol)` for every non-test production reference to a readiness
/// symbol. The DEFINING files are NOT skipped — they are scanned with the
/// definition-scoped scanner ([`readiness_defining_file_violations`]), which
/// subtracts only the sanctioned definition sites + internal wiring and reports
/// any same-file production caller. Non-defining files use the whole-file scan
/// (any reference is a violation there).
fn readiness_production_reference_sites() -> Vec<(String, String)> {
    let src_root = crate_root().join("src");
    let mut out = Vec::new();
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
        if is_readiness_test_file(&rel) {
            continue;
        }
        let src = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        // Cheap pre-reject: a file mentioning none of the symbols as a substring
        // cannot reference one as a token.
        if !NODE_DOMAIN_READINESS_SYMBOLS
            .iter()
            .any(|s| src.contains(s))
        {
            continue;
        }
        let symbols = if READINESS_DEFINING_FILES.contains(&rel.as_str()) {
            // Defining file: scan its body, subtracting the sanctioned definition
            // sites; a same-file production caller is reported.
            readiness_defining_file_violations(&src)
        } else {
            // Consumer file: any non-test reference is a production caller.
            readiness_symbols_referenced_in(&src)
        };
        for sym in symbols {
            out.push((rel.clone(), sym));
        }
    }
    out.sort();
    out
}

#[test]
fn node_domain_readiness_primitives_have_zero_production_callers() {
    let sites = readiness_production_reference_sites();
    assert!(
        sites.is_empty(),
        "ZERO-PRODUCTION-CALLER FENCE: a node-domain readiness primitive is referenced in non-test \
         production source outside its sanctioned definition sites (a same-file caller inside a \
         defining file fires too — only the seven symbols' own definitions + internal wiring are \
         subtracted). These seven primitives realize their facts via materialize-then-predicate \
         (zero-drift now) and MUST NOT be wired into a production decision path until the bottom-up \
         node-domain realization replaces them (tracked in docs/arch/parselower-design.md). Keep \
         the Kind-B sites on `legacy_semantic_type_expr_bridge`. Offending references:\n  {}",
        sites
            .iter()
            .map(|(rel, sym)| format!("{rel}: {sym}"))
            .collect::<Vec<_>>()
            .join("\n  ")
    );
}

#[test]
fn node_domain_readiness_fence_self_test_discriminates() {
    // FIRE (RED): a production fn body that CALLS one of the seven symbols is a
    // production reference site.
    let production_call = r#"
        fn some_consumer(ctx: &C, node: SemanticNodeId) -> bool {
            node_can_shell_raise(ctx, node)
        }
    "#;
    assert!(
        readiness_symbols_referenced_in(production_call).contains("node_can_shell_raise"),
        "self-test: a production call to `node_can_shell_raise` MUST be reported; got {:?}",
        readiness_symbols_referenced_in(production_call)
    );

    // FIRE (RED): a production struct FIELD typed as `NodeBearingExpansion` is a
    // production reference site (the whole-subtree ident scan, not body-only).
    let production_field = r#"
        struct ConsumerState {
            pending: NodeBearingExpansion,
        }
    "#;
    assert!(
        readiness_symbols_referenced_in(production_field).contains("NodeBearingExpansion"),
        "self-test: a production struct field typed `NodeBearingExpansion` MUST be reported; got \
         {:?}",
        readiness_symbols_referenced_in(production_field)
    );

    // PASS: a reference confined to a `#[cfg(test)]` module is test-only and MUST
    // NOT fire (the test-gated subtree is skipped wholesale).
    let test_only = r#"
        #[cfg(test)]
        mod t {
            fn exercise(ctx: &C, node: SemanticNodeId) -> bool {
                node_can_shell_raise(ctx, node)
                    && raised_shape_eq_nodes(ctx, node, node).is_some()
            }
        }
    "#;
    assert!(
        readiness_symbols_referenced_in(test_only).is_empty(),
        "self-test: references confined to a `#[cfg(test)]` module are test-only and MUST NOT fire; \
         got {:?}",
        readiness_symbols_referenced_in(test_only)
    );

    // PASS: a `#[cfg(test)]`-gated free fn is likewise excluded.
    let test_gated_fn = r#"
        #[cfg(test)]
        fn exercise(ctx: &C, node: SemanticNodeId) -> Option<bool> {
            raised_shape_eq_node_type_expr(ctx, node, &e)
        }
    "#;
    assert!(
        readiness_symbols_referenced_in(test_gated_fn).is_empty(),
        "self-test: a `#[cfg(test)]`-gated free fn referencing a symbol MUST NOT fire; got {:?}",
        readiness_symbols_referenced_in(test_gated_fn)
    );

    // PASS: production source mentioning NONE of the symbols is clean.
    let clean = r#"
        fn unrelated(x: u32) -> u32 { x + 1 }
    "#;
    assert!(
        readiness_symbols_referenced_in(clean).is_empty(),
        "self-test: unrelated production source MUST NOT fire; got {:?}",
        readiness_symbols_referenced_in(clean)
    );

    // The substring-vs-token distinction: a DIFFERENT identifier that merely
    // CONTAINS a symbol name as a substring (e.g. `node_can_shell_raise_helper`)
    // is a distinct whole identifier and MUST NOT be reported.
    let substring_decoy = r#"
        fn node_can_shell_raise_helper(x: u32) -> u32 { x }
    "#;
    assert!(
        !readiness_symbols_referenced_in(substring_decoy).contains("node_can_shell_raise"),
        "self-test: a distinct identifier merely CONTAINING a symbol name as a substring MUST NOT \
         be reported (whole-identifier scan); got {:?}",
        readiness_symbols_referenced_in(substring_decoy)
    );

    // r2 — FIRE (RED): a readiness ident inside a MACRO invocation's token tree
    // (`some_macro!(... node_can_shell_raise(...) ...)`) is a production reference.
    // The default `syn` walk does not descend into a macro's opaque token stream,
    // so the old scan missed it; the recursive macro-token scan now reports it.
    let macro_token_ref = r#"
        fn some_consumer(ctx: &C, node: SemanticNodeId) -> bool {
            some_macro!(if node_can_shell_raise(ctx, node) { yes } else { no })
        }
    "#;
    assert!(
        readiness_symbols_referenced_in(macro_token_ref).contains("node_can_shell_raise"),
        "self-test [r2]: a readiness ident inside a macro token tree MUST be reported (macro-token \
         scan); got {:?}",
        readiness_symbols_referenced_in(macro_token_ref)
    );

    // r2 — FIRE (RED): a readiness ident nested inside a GROUP within macro tokens
    // (the recursion must descend `Group` streams).
    let macro_token_nested = r#"
        fn some_consumer(ctx: &C, node: SemanticNodeId) -> bool {
            outer_macro!([ inner( raised_shape_eq_nodes(ctx, node, node) ) ])
        }
    "#;
    assert!(
        readiness_symbols_referenced_in(macro_token_nested).contains("raised_shape_eq_nodes"),
        "self-test [r2]: a readiness ident nested in a macro-token Group MUST be reported (the scan \
         descends groups); got {:?}",
        readiness_symbols_referenced_in(macro_token_nested)
    );

    // r2 — PASS: a readiness symbol appearing only as TEXT inside a string literal
    // within macro tokens is NOT a hit (a string is a `Literal` token, never an
    // `Ident`). Proves the macro scan is token-typed, not substring.
    let macro_string_literal = r#"
        fn log_it() {
            println!("node_can_shell_raise is fenced");
        }
    "#;
    assert!(
        !readiness_symbols_referenced_in(macro_string_literal).contains("node_can_shell_raise"),
        "self-test [r2]: a readiness symbol appearing only as string-literal TEXT inside macro \
         tokens MUST NOT be reported (Ident-typed scan, not substring); got {:?}",
        readiness_symbols_referenced_in(macro_string_literal)
    );

    // r2 — PASS: a `#[cfg(test)] use …node_can_shell_raise…;` import is test code
    // and MUST NOT fire. The cfg classifier now inspects `Item::Use` attrs, so the
    // test-gated import is excluded (it was wrongly scanned before — R3-10).
    let cfg_test_use = r#"
        #[cfg(test)]
        use crate::project_semantic_dispatch::raise::node_can_shell_raise;
    "#;
    assert!(
        readiness_symbols_referenced_in(cfg_test_use).is_empty(),
        "self-test [r2]: a `#[cfg(test)] use …node_can_shell_raise…;` MUST NOT fire (Item::Use is \
         now cfg-gated); got {:?}",
        readiness_symbols_referenced_in(cfg_test_use)
    );

    // r2 — FIRE (RED): a PRODUCTION (non-test) `use …node_can_shell_raise…;`
    // import IS a production reference (importing the symbol into a production
    // namespace). Proves the Item::Use cfg-gating does not BLANKET-exclude `use`
    // items — only test-gated ones.
    let prod_use = r#"
        use crate::project_semantic_dispatch::raise::node_can_shell_raise;
    "#;
    assert!(
        readiness_symbols_referenced_in(prod_use).contains("node_can_shell_raise"),
        "self-test [r2]: a PRODUCTION `use …node_can_shell_raise…;` MUST fire (only test-gated use \
         items are excluded); got {:?}",
        readiness_symbols_referenced_in(prod_use)
    );
}

#[test]
fn readiness_defining_file_scan_discriminates_same_file_caller() {
    // CF1: the defining-file scan must report a SAME-FILE production caller while
    // passing the symbols' own definitions + their internal wiring.

    // PASS: a synthetic defining file holding ONLY the definitions + sanctioned
    // internal wiring — the classifier `fn` definitions, the equality primitive
    // calling `node_can_shell_raise` internally, the artifact `struct`, its
    // `impl … { fn new() -> Self }` constructor, and the facade `fn` taking
    // `&NodeBearingExpansion` — produces ZERO violations.
    let definitions_only = r#"
        pub(crate) fn node_can_shell_raise(ctx: &C, node: SemanticNodeId) -> bool { true }
        pub(crate) fn node_contains_semantic_miss_legacy_equivalent(ctx: &C, n: SemanticNodeId) -> bool { false }
        pub(crate) fn node_is_expanded_surface_legacy_equivalent(ctx: &C, n: SemanticNodeId) -> bool { false }
        pub(crate) fn raised_shape_eq_node_type_expr(ctx: &C, n: SemanticNodeId, e: &E) -> Option<bool> { None }
        pub(crate) fn raised_shape_eq_nodes(ctx: &C, a: SemanticNodeId, b: SemanticNodeId) -> Option<bool> {
            // INTERNAL wiring: the equality primitive (one of the seven) calling
            // `node_can_shell_raise` (another of the seven) is sanctioned.
            if a == b { return node_can_shell_raise(ctx, a).then_some(true); }
            None
        }
        pub(crate) struct NodeBearingExpansion { node: SemanticNodeId }
        impl NodeBearingExpansion {
            pub(crate) fn new(node: SemanticNodeId) -> Self { Self { node } }
        }
        pub(crate) fn materialize_node_bearing_expansion(d: &D, artifact: &NodeBearingExpansion) -> Option<X> {
            None
        }
        // An unrelated module-private helper that references NONE of the seven.
        fn unrelated(x: u32) -> u32 { x + 1 }
    "#;
    assert!(
        readiness_defining_file_violations(definitions_only).is_empty(),
        "self-test [CF1]: the definitions + sanctioned internal wiring MUST produce ZERO \
         violations; got {:?}",
        readiness_defining_file_violations(definitions_only)
    );

    // FIRE (RED): the SAME definitions PLUS a separate production caller of a
    // classifier (a fn that is NOT one of the seven) — the whole-file-exclusion
    // hole (claude Plant 5). The scan MUST report the caller.
    let with_same_file_caller = r#"
        pub(crate) fn node_can_shell_raise(ctx: &C, node: SemanticNodeId) -> bool { true }
        pub(crate) struct NodeBearingExpansion { node: SemanticNodeId }
        impl NodeBearingExpansion {
            pub(crate) fn new(node: SemanticNodeId) -> Self { Self { node } }
        }
        // A REAL production caller INSIDE the defining file — invisible under the
        // old whole-file exclusion.
        pub(crate) fn some_production_consumer(ctx: &C, node: SemanticNodeId) -> bool {
            node_can_shell_raise(ctx, node)
        }
    "#;
    assert!(
        readiness_defining_file_violations(with_same_file_caller).contains("node_can_shell_raise"),
        "self-test [CF1]: a same-file production caller of `node_can_shell_raise` inside a defining \
         file MUST be reported (the whole-file-exclusion hole); got {:?}",
        readiness_defining_file_violations(with_same_file_caller)
    );

    // FIRE (RED): a same-file production consumer that takes the artifact TYPE
    // (`NodeBearingExpansion`) outside the sanctioned definition sites is also a
    // production reference.
    let consumer_holds_artifact = r#"
        pub(crate) struct NodeBearingExpansion { node: SemanticNodeId }
        impl NodeBearingExpansion {
            pub(crate) fn new(node: SemanticNodeId) -> Self { Self { node } }
        }
        pub(crate) struct ProductionState {
            pending: NodeBearingExpansion,
        }
    "#;
    assert!(
        readiness_defining_file_violations(consumer_holds_artifact)
            .contains("NodeBearingExpansion"),
        "self-test [CF1]: a production struct field typed `NodeBearingExpansion` outside the \
         sanctioned sites MUST be reported; got {:?}",
        readiness_defining_file_violations(consumer_holds_artifact)
    );

    // PASS: a same-file caller confined to a `#[cfg(test)]` module is test code
    // and MUST NOT fire (the EXACT-test-gated subtree is skipped).
    let test_caller_in_defining_file = r#"
        pub(crate) fn node_can_shell_raise(ctx: &C, node: SemanticNodeId) -> bool { true }
        #[cfg(test)]
        mod t {
            fn exercise(ctx: &C, node: SemanticNodeId) -> bool {
                super::node_can_shell_raise(ctx, node)
            }
        }
    "#;
    assert!(
        readiness_defining_file_violations(test_caller_in_defining_file).is_empty(),
        "self-test [CF1]: a same-file caller confined to `#[cfg(test)]` MUST NOT fire; got {:?}",
        readiness_defining_file_violations(test_caller_in_defining_file)
    );
}

#[test]
fn readiness_defining_file_scan_treats_impl_as_whole_item_minus_sanctioned_spans() {
    // The defining-file scan runs each `syn::Item::Impl` through the SAME
    // whole-item identifier walk as non-impl items, subtracting ONLY the exact
    // sanctioned definition spans (cfg(test) impl-items; readiness-named
    // impl-items; the self_ty of an INHERENT readiness impl). This consolidated
    // fixture pins every impl scope: method BODY, trait path / self_ty / generic
    // header reference, WHERE-clause bound, ATTRIBUTE, the sanctioned inherent
    // self-type header, and a sanctioned associated definition. The where-clause /
    // attribute / trait-impl-self_ty cases are RED against the prior per-piece
    // (items + header-token) scanner that never inspected them — see the discrete
    // discrimination asserts below.

    // FIRE: a method BODY caller inside `impl NodeBearingExpansion { fn leaks() {
    // node_can_shell_raise(..) } }` IS reported. The constructor `new` (NOT a
    // readiness name) is SCANNED and contributes nothing, so the only hit is the
    // body call — proving the walk runs on bodies, not a wholesale impl skip.
    let method_body_caller = r#"
        pub(crate) struct NodeBearingExpansion { node: SemanticNodeId }
        impl NodeBearingExpansion {
            pub(crate) fn new(node: SemanticNodeId) -> Self { Self { node } }
            pub(crate) fn leaks(&self, ctx: &C) -> bool {
                node_can_shell_raise(ctx, self.node)
            }
        }
    "#;
    assert!(
        readiness_defining_file_violations(method_body_caller).contains("node_can_shell_raise"),
        "self-test: a method-BODY caller inside `impl NodeBearingExpansion {{ fn leaks() {{ \
         node_can_shell_raise(..) }} }}` MUST be reported; got {:?}",
        readiness_defining_file_violations(method_body_caller)
    );

    // FIRE: a NON-sanctioned impl's trait path / self_ty / GENERIC header
    // reference IS reported. `NodeBearingExpansion` appears only as a trait
    // type-argument (`Marker<NodeBearingExpansion>`) on `Holder<u8>`, body EMPTY —
    // the whole-impl walk descends the trait path so the hit fires.
    let header_generic_ref = r#"
        struct Holder<T>(T);
        trait Marker<T> {}
        impl Marker<NodeBearingExpansion> for Holder<u8> {}
    "#;
    assert!(
        readiness_defining_file_violations(header_generic_ref).contains("NodeBearingExpansion"),
        "self-test: a non-sanctioned impl HEADER reference \
         (`impl Marker<NodeBearingExpansion> for Holder<u8>`) MUST be reported; got {:?}",
        readiness_defining_file_violations(header_generic_ref)
    );

    // FIRE: a WHERE-clause reference IS reported. `NodeBearingExpansion` appears
    // ONLY in the where-clause bound (`T: Bound<NodeBearingExpansion>`), body
    // EMPTY — covered BY CONSTRUCTION because the whole-impl walk descends
    // `generics` (which carries the where-clause). The prior per-piece scanner
    // (items + a header token scan over trait path / self_ty / generics-as-tokens)
    // is the discrimination baseline; this case is the where-clause RED proof.
    let where_clause_ref = r#"
        struct Holder<T>(T);
        trait Marker {}
        trait Bound<U> {}
        impl<T> Marker for Holder<T> where T: Bound<NodeBearingExpansion> {}
    "#;
    assert!(
        readiness_defining_file_violations(where_clause_ref).contains("NodeBearingExpansion"),
        "self-test: a WHERE-clause reference \
         (`impl<T> Marker for Holder<T> where T: Bound<NodeBearingExpansion> {{}}`) MUST be \
         reported (the whole-impl walk descends the where-clause); got {:?}",
        readiness_defining_file_violations(where_clause_ref)
    );

    // FIRE: an ATTRIBUTE reference IS reported. `NodeBearingExpansion` appears ONLY
    // inside an outer attribute on the impl (`#[some_attr(NodeBearingExpansion)]`),
    // body EMPTY — covered BY CONSTRUCTION because the whole-impl walk descends
    // `attrs`. A non-test attr's tokens are real production tokens (the symbol is
    // an `Ident`, not string text).
    let attribute_ref = r#"
        struct Holder<T>(T);
        #[some_attr(NodeBearingExpansion)]
        impl Holder<u8> {}
    "#;
    assert!(
        readiness_defining_file_violations(attribute_ref).contains("NodeBearingExpansion"),
        "self-test: an ATTRIBUTE reference (`#[some_attr(NodeBearingExpansion)] impl Holder<u8> \
         {{}}`) MUST be reported (the whole-impl walk descends attrs); got {:?}",
        readiness_defining_file_violations(attribute_ref)
    );

    // PASS: the sanctioned INHERENT self-type header `impl NodeBearingExpansion {
    // fn new() -> Self {} }` is NOT reported. The self_ty occurrence is subtracted
    // (the artifact naming itself in its own inherent header); the `new` body
    // references no readiness token. ZERO violations.
    let sanctioned_inherent_self_ty = r#"
        pub(crate) struct NodeBearingExpansion { node: SemanticNodeId }
        impl NodeBearingExpansion {
            pub(crate) fn new(node: SemanticNodeId) -> Self { Self { node } }
        }
    "#;
    assert!(
        readiness_defining_file_violations(sanctioned_inherent_self_ty).is_empty(),
        "self-test: a sanctioned INHERENT `impl NodeBearingExpansion {{ fn new() -> Self {{}} }}` \
         header MUST NOT fire (only the HEAD self_ty occurrence is subtracted); got {:?}",
        readiness_defining_file_violations(sanctioned_inherent_self_ty)
    );

    // FIRE (G2 — inherent self_ty GENERIC ARGUMENT is NOT exempt): an INHERENT
    // `impl NodeBearingExpansion<NodeBearingExpansion<()>>` names the artifact at
    // the self-type HEAD (sanctioned, exempt) AND uses `NodeBearingExpansion` as a
    // GENERIC ARGUMENT on that self-type (the INNER occurrence). The inner
    // occurrence is a production reference and MUST be reported; only the single
    // head self-naming occurrence is subtracted. RED against the prior
    // whole-self_ty-skip (which dropped the ENTIRE self_ty subtree, hiding the
    // inner reference). CRUCIAL for the RED-discrimination: the body references NO
    // readiness token AT ALL (a bare `u32` constructor) — so the ONLY possible hit
    // is the self_ty's inner generic argument. The prior whole-self_ty-skip visited
    // only attrs/generics/items (never the self_ty), so it saw ZERO readiness
    // tokens here and would have PASSED (no violation); the head-only-subtraction
    // visits the self_ty and reports the inner occurrence.
    let inherent_self_ty_generic_arg = r#"
        impl NodeBearingExpansion<NodeBearingExpansion<()>> {
            pub(crate) fn count(&self) -> u32 { 0 }
        }
    "#;
    assert!(
        readiness_defining_file_violations(inherent_self_ty_generic_arg)
            .contains("NodeBearingExpansion"),
        "self-test [G2]: a readiness symbol used as a GENERIC ARGUMENT on an inherent readiness \
         self-type (`impl NodeBearingExpansion<NodeBearingExpansion<()>>`) MUST be reported — only \
         the HEAD self-naming occurrence is exempt; got {:?}",
        readiness_defining_file_violations(inherent_self_ty_generic_arg)
    );

    // PASS: a sanctioned ASSOCIATED definition — an impl-item whose OWN NAME is a
    // readiness symbol (a readiness fn relocated as a method) — is NOT reported;
    // its own subtree is subtracted (matching the module-scope sanctioned-def
    // behaviour). Here the body references no OTHER readiness symbol. ZERO
    // violations. NOTE: per the by-span subtraction, a reference inside the
    // sanctioned assoc item's OWN body is NOT reported (it is the def's internal
    // wiring), consistent with how a top-level readiness def's own body is
    // subtracted at module scope — see the `with_other_readiness_in_assoc_body`
    // proof below.
    let sanctioned_assoc_def = r#"
        impl Dispatch {
            pub(crate) fn raised_shape_eq_nodes(&self, a: SemanticNodeId, b: SemanticNodeId) -> bool {
                a == b
            }
        }
    "#;
    assert!(
        readiness_defining_file_violations(sanctioned_assoc_def).is_empty(),
        "self-test: a sanctioned ASSOCIATED definition (impl-item whose OWN NAME is a readiness \
         symbol) MUST NOT fire when its body references no OTHER readiness symbol; got {:?}",
        readiness_defining_file_violations(sanctioned_assoc_def)
    );

    // PASS (by-span consistency): a DIFFERENT readiness symbol referenced INSIDE
    // the sanctioned assoc item's own body is NOT reported — the sanctioned assoc
    // item's whole subtree is subtracted by span, exactly as a top-level readiness
    // def's own body is. Documents the module-scope-consistent behaviour the brief
    // calls out. (A reference OUTSIDE the subtree would still fire — covered by the
    // method_body_caller case above.)
    let with_other_readiness_in_assoc_body = r#"
        impl Dispatch {
            pub(crate) fn raised_shape_eq_nodes(&self, ctx: &C, a: SemanticNodeId) -> bool {
                node_can_shell_raise(ctx, a)
            }
        }
    "#;
    assert!(
        readiness_defining_file_violations(with_other_readiness_in_assoc_body).is_empty(),
        "self-test: a readiness symbol referenced INSIDE a sanctioned assoc item's OWN body is its \
         internal wiring and MUST NOT fire (by-span subtraction, module-scope-consistent); got {:?}",
        readiness_defining_file_violations(with_other_readiness_in_assoc_body)
    );

    // PASS (test-gated impl method): a `#[cfg(test)]`-gated impl method calling a
    // readiness fn is test code and MUST NOT fire (the cfg(test) impl-item span is
    // not descended).
    let test_gated_impl_method = r#"
        impl NodeBearingExpansion {
            #[cfg(test)]
            fn exercise(&self, ctx: &C) -> bool { node_can_shell_raise(ctx, self.node) }
        }
    "#;
    assert!(
        readiness_defining_file_violations(test_gated_impl_method).is_empty(),
        "self-test: a `#[cfg(test)]`-gated impl method referencing a readiness symbol MUST NOT \
         fire; got {:?}",
        readiness_defining_file_violations(test_gated_impl_method)
    );

    // FIRE (trait-impl self_ty is NOT exempt): a readiness type as the self_ty of
    // a TRAIT impl (`impl SomeTrait for NodeBearingExpansion {}`) is production
    // wiring and MUST be reported. The self_ty subtraction is ONLY for an INHERENT
    // impl; the whole-impl walk descends a trait impl's self_ty. This is the
    // trait-impl-self_ty RED proof the prior mechanism (which exempted any impl
    // whose self_ty last-ident was a readiness symbol, trait or not) would MISS.
    let trait_impl_self_ty = r#"
        trait SomeTrait {}
        impl SomeTrait for NodeBearingExpansion {}
    "#;
    assert!(
        readiness_defining_file_violations(trait_impl_self_ty).contains("NodeBearingExpansion"),
        "self-test: a readiness type as a TRAIT impl's self_ty \
         (`impl SomeTrait for NodeBearingExpansion`) MUST be reported (self_ty exemption is \
         INHERENT-only); got {:?}",
        readiness_defining_file_violations(trait_impl_self_ty)
    );
}

#[test]
fn readiness_impl_scan_discrimination_baseline() {
    // Discrimination proof for the three cases the PRIOR per-piece mechanism
    // missed (where-clause, attribute, trait-impl self_ty). The prior mechanism
    // scanned an impl by (a) per-impl-ITEM bodies and (b) a header token scan over
    // ONLY `trait_` path + `self_ty` + `generics`-AS-A-TOKEN-STREAM, and exempted
    // ANY impl (trait or inherent) whose self_ty last-ident was a readiness
    // symbol. We SIMULATE the relevant misses of that mechanism inline and assert
    // the WHOLE-impl walk reports each — so each fixture is RED against a
    // mechanism that would miss it.

    // (1) where-clause: simulate the prior header token scan that walked ONLY the
    // trait path + self_ty (NOT generics/where-clause tokens). Build the impl,
    // collect idents from trait-path + self_ty ONLY (the narrowest prior-shape
    // miss), and confirm it does NOT see the where-clause symbol — while the
    // whole-impl walk DOES.
    let where_src = r#"
        struct Holder<T>(T);
        trait Marker {}
        trait Bound<U> {}
        impl<T> Marker for Holder<T> where T: Bound<NodeBearingExpansion> {}
    "#;
    let file = syn::parse_file(where_src).expect("parse where_src");
    let impl_item = file
        .items
        .iter()
        .find_map(|it| match it {
            syn::Item::Impl(i) => Some(i),
            _ => None,
        })
        .expect("find impl");
    // Prior-shape SIM: trait path + self_ty tokens ONLY.
    let mut prior_sim = BTreeSet::new();
    {
        use quote::ToTokens;
        if let Some((_, trait_path, _)) = &impl_item.trait_ {
            scan_macro_tokens_for_idents(&trait_path.to_token_stream(), &mut prior_sim);
        }
        scan_macro_tokens_for_idents(&impl_item.self_ty.to_token_stream(), &mut prior_sim);
    }
    assert!(
        !prior_sim.contains("NodeBearingExpansion"),
        "discrimination baseline: a trait-path+self_ty-only header sim MUST MISS the where-clause \
         symbol (proves the fixture is RED against that shape); got {prior_sim:?}"
    );
    assert!(
        readiness_defining_file_violations(where_src).contains("NodeBearingExpansion"),
        "discrimination baseline: the whole-impl walk MUST report the where-clause symbol; got {:?}",
        readiness_defining_file_violations(where_src)
    );

    // (2) attribute: the prior per-impl mechanism never scanned the impl's OUTER
    // attrs at all (it scanned items + a trait/self_ty/generics header token
    // scan). SIM that by collecting idents from trait path + self_ty + generics
    // (the prior header scan's WIDEST shape) and confirm it misses an attr-only
    // symbol — while the whole-impl walk reports it.
    let attr_src = r#"
        struct Holder<T>(T);
        #[some_attr(NodeBearingExpansion)]
        impl Holder<u8> {}
    "#;
    let attr_file = syn::parse_file(attr_src).expect("parse attr_src");
    let attr_impl = attr_file
        .items
        .iter()
        .find_map(|it| match it {
            syn::Item::Impl(i) => Some(i),
            _ => None,
        })
        .expect("find attr impl");
    let mut attr_prior_sim = BTreeSet::new();
    {
        use quote::ToTokens;
        if let Some((_, trait_path, _)) = &attr_impl.trait_ {
            scan_macro_tokens_for_idents(&trait_path.to_token_stream(), &mut attr_prior_sim);
        }
        scan_macro_tokens_for_idents(&attr_impl.self_ty.to_token_stream(), &mut attr_prior_sim);
        scan_macro_tokens_for_idents(&attr_impl.generics.to_token_stream(), &mut attr_prior_sim);
    }
    assert!(
        !attr_prior_sim.contains("NodeBearingExpansion"),
        "discrimination baseline: the prior header token scan (trait/self_ty/generics, NO attrs) \
         MUST MISS an attribute-only symbol; got {attr_prior_sim:?}"
    );
    assert!(
        readiness_defining_file_violations(attr_src).contains("NodeBearingExpansion"),
        "discrimination baseline: the whole-impl walk MUST report the attribute symbol; got {:?}",
        readiness_defining_file_violations(attr_src)
    );

    // (3) trait-impl self_ty: the prior mechanism exempted ANY impl whose self_ty
    // last-ident was a readiness symbol (trait or inherent), so a TRAIT impl with
    // a readiness self_ty was treated as the artifact's own def and DROPPED. SIM
    // the prior exemption predicate, confirm it would have exempted this trait
    // impl, and confirm the whole-impl walk REPORTS it (self_ty exemption is now
    // INHERENT-only).
    let trait_self_ty_src = r#"
        trait SomeTrait {}
        impl SomeTrait for NodeBearingExpansion {}
    "#;
    let ts_file = syn::parse_file(trait_self_ty_src).expect("parse trait_self_ty_src");
    let ts_impl = ts_file
        .items
        .iter()
        .find_map(|it| match it {
            syn::Item::Impl(i) => Some(i),
            _ => None,
        })
        .expect("find trait-self_ty impl");
    // Prior exemption predicate: self_ty last-ident is a readiness symbol,
    // IGNORING whether it is a trait impl.
    let prior_would_exempt = impl_self_ty_last_ident_local(&ts_impl.self_ty)
        .as_deref()
        .is_some_and(|s| NODE_DOMAIN_READINESS_SYMBOLS.contains(&s));
    assert!(
        prior_would_exempt,
        "discrimination baseline: the prior self_ty-only exemption WOULD have exempted this trait \
         impl (proving the fixture is RED against that shape)"
    );
    assert!(
        readiness_defining_file_violations(trait_self_ty_src).contains("NodeBearingExpansion"),
        "discrimination baseline: the whole-impl walk MUST report a TRAIT impl's readiness self_ty \
         (the self_ty exemption is INHERENT-only); got {:?}",
        readiness_defining_file_violations(trait_self_ty_src)
    );
}

#[test]
fn readiness_attribute_meta_list_scanned_on_all_scan_paths() {
    // G1: the `visit_meta_list` attribute-token descent must apply CONSISTENTLY on
    // ALL THREE scan paths (whole-file, per-item, whole-impl), not only inside an
    // impl. A NON-impl item (a free `fn`) carrying a readiness symbol in an
    // ATTRIBUTE argument list (`#[some_attr(node_can_shell_raise)]`) is
    // `syn`-visible but was MISSED before this fix, because `syn` routes a
    // `Meta::List`'s tokens to the no-op `visit_token_stream` and the non-impl
    // scanners did NOT override `visit_meta_list`.

    // FIRE (RED via the WHOLE-FILE scan `readiness_production_idents`): a
    // module-scope free fn with a readiness symbol in its attribute meta-list IS
    // reported. The fn body references NO readiness token, so the ONLY hit is the
    // attribute argument — proving the meta-list descent (not a body/signature
    // reference) is what fires.
    let non_impl_attr_whole_file = r#"
        #[some_attr(node_can_shell_raise)]
        fn production_consumer(x: u32) -> u32 { x + 1 }
    "#;
    assert!(
        readiness_symbols_referenced_in(non_impl_attr_whole_file).contains("node_can_shell_raise"),
        "self-test [G1]: a NON-impl item's attribute meta-list ident \
         (`#[some_attr(node_can_shell_raise)] fn production_consumer() {{}}`) MUST be reported by \
         the whole-file scan (the shared `visit_meta_list` descent); got {:?}",
        readiness_symbols_referenced_in(non_impl_attr_whole_file)
    );

    // FIRE (RED via the PER-ITEM defining-file scan `readiness_production_idents_of_item`,
    // reached through `readiness_defining_file_violations` → `scan_defining_items`):
    // the SAME non-impl attr case, routed through the per-item walker that scans
    // each non-impl, non-definition item. RED against the prior per-item scanner,
    // which also lacked `visit_meta_list`.
    assert!(
        readiness_defining_file_violations(non_impl_attr_whole_file)
            .contains("node_can_shell_raise"),
        "self-test [G1]: a NON-impl item's attribute meta-list ident MUST be reported by the \
         per-item defining-file scan (the shared `visit_meta_list` descent); got {:?}",
        readiness_defining_file_violations(non_impl_attr_whole_file)
    );

    // RED-DISCRIMINATION: simulate the PRE-fix non-impl scanner (a `syn::Visit`
    // with `visit_ident` + `visit_macro` but NO `visit_meta_list` override — the
    // exact prior shape of `readiness_production_idents`) and confirm it MISSES the
    // attribute meta-list ident (proving the fixture is RED against that shape),
    // while the production shared scan REPORTS it.
    struct PreFixNoMetaList {
        idents: BTreeSet<String>,
    }
    impl<'ast> Visit<'ast> for PreFixNoMetaList {
        fn visit_ident(&mut self, id: &'ast proc_macro2::Ident) {
            self.idents.insert(id.to_string());
        }
        fn visit_macro(&mut self, m: &'ast syn::Macro) {
            scan_macro_tokens_for_idents(&m.tokens, &mut self.idents);
            syn::visit::visit_macro(self, m);
        }
        // NO visit_meta_list override — `Meta::List` tokens route to the no-op
        // `visit_token_stream` (the pre-fix blind spot).
    }
    let file = syn::parse_file(non_impl_attr_whole_file).expect("parse non_impl_attr_whole_file");
    let mut pre_fix = PreFixNoMetaList {
        idents: BTreeSet::new(),
    };
    pre_fix.visit_file(&file);
    assert!(
        !pre_fix.idents.contains("node_can_shell_raise"),
        "self-test [G1] discrimination: the PRE-fix non-impl scanner (no `visit_meta_list`) MUST \
         MISS the attribute meta-list ident — proving the fixture is RED against that shape; got \
         {:?}",
        pre_fix.idents
    );

    // PASS (already-covered impl-attr case still works): the impl-path attr
    // meta-list descent is preserved by the shared visitor. An impl carrying a
    // readiness symbol ONLY in an outer attribute IS still reported.
    let impl_attr = r#"
        struct Holder<T>(T);
        #[some_attr(NodeBearingExpansion)]
        impl Holder<u8> {}
    "#;
    assert!(
        readiness_defining_file_violations(impl_attr).contains("NodeBearingExpansion"),
        "self-test [G1]: the existing impl-attribute meta-list case MUST still fire after factoring \
         `visit_meta_list` into the shared visitor; got {:?}",
        readiness_defining_file_violations(impl_attr)
    );

    // PASS: a readiness symbol appearing ONLY as STRING-LITERAL text inside a
    // non-impl item's `Meta::List` attribute (`#[some_attr("node_can_shell_raise")]`)
    // is NOT a hit (string text is a `Literal`, never an `Ident`) — the meta-list
    // descent stays token-typed, not substring.
    let attr_string_literal = r#"
        #[some_attr("node_can_shell_raise is fenced")]
        fn documented(x: u32) -> u32 { x }
    "#;
    assert!(
        !readiness_symbols_referenced_in(attr_string_literal).contains("node_can_shell_raise"),
        "self-test [G1]: a readiness symbol appearing only as string-literal TEXT in a non-impl \
         `Meta::List` attribute MUST NOT be reported (Ident-typed meta-list scan); got {:?}",
        readiness_symbols_referenced_in(attr_string_literal)
    );
}

#[test]
fn readiness_cfg_classifier_is_exact_test_only() {
    // CF2: the readiness fence's cfg classifier must classify as test-gated ONLY
    // the EXACT test/test-support shapes, and as PRODUCTION (therefore scanned)
    // every production-satisfiable cfg — most importantly `#[cfg(not(test))]`.
    // Build the attribute list from synthetic source so the classifier sees what
    // `attr.meta` carries.
    fn attrs(src: &str) -> Vec<syn::Attribute> {
        let f: syn::File = syn::parse_str(&format!("{src}\nfn __probe() {{}}"))
            .expect("parse synthetic cfg attribute");
        f.items
            .iter()
            .find_map(|it| match it {
                syn::Item::Fn(func) if func.sig.ident == "__probe" => Some(func.attrs.clone()),
                _ => None,
            })
            .expect("find __probe fn")
    }

    // Test-gated (skipped): bare `cfg(test)`.
    assert!(
        readiness_attrs_are_cfg_test(&attrs("#[cfg(test)]")),
        "self-test [CF2]: `#[cfg(test)]` MUST classify as test-gated (skipped)"
    );
    // Test-gated (skipped): the canonical `any(test, feature = \"test-support\")`.
    assert!(
        readiness_attrs_are_cfg_test(&attrs("#[cfg(any(test, feature = \"test-support\"))]")),
        "self-test [CF2]: `#[cfg(any(test, feature = \"test-support\"))]` MUST classify as test-gated"
    );

    // PRODUCTION (scanned): `#[cfg(not(test))]` — the LOAD-BEARING fix. The
    // shallow `mentions-test` matcher saw the bare `test` ident inside `not(test)`
    // and wrongly skipped a `cfg(not(test))` production item.
    assert!(
        !readiness_attrs_are_cfg_test(&attrs("#[cfg(not(test))]")),
        "self-test [CF2]: `#[cfg(not(test))]` MUST classify as PRODUCTION (scanned) — it is present \
         in EVERY non-test build; the shallow matcher's hole"
    );
    // PRODUCTION (scanned): `#[cfg(any(test, debug_assertions))]` —
    // `debug_assertions` is ON in ordinary debug builds.
    assert!(
        !readiness_attrs_are_cfg_test(&attrs("#[cfg(any(test, debug_assertions))]")),
        "self-test [CF2]: `#[cfg(any(test, debug_assertions))]` MUST classify as PRODUCTION — \
         debug-build-reachable"
    );
    // PRODUCTION (scanned): `#[cfg(all(test, unix))]` — genuinely test-only by
    // entailment, but NOT the EXACT canonical gate, so the strict recogniser
    // treats it as not-exactly-test (production-side) and SCANS it. A production
    // caller cannot legitimately hide behind a non-canonical conjunction here.
    assert!(
        !readiness_attrs_are_cfg_test(&attrs("#[cfg(all(test, unix))]")),
        "self-test [CF2]: `#[cfg(all(test, unix))]` is NOT the EXACT canonical test gate ⇒ scanned"
    );
    // PRODUCTION (scanned): a lone `feature` gate that does not name `test`.
    assert!(
        !readiness_attrs_are_cfg_test(&attrs("#[cfg(feature = \"oracle-gen\")]")),
        "self-test [CF2]: a feature-only cfg that does not name `test` MUST classify as PRODUCTION"
    );
    // PRODUCTION (scanned): no cfg at all.
    assert!(
        !readiness_attrs_are_cfg_test(&attrs("#[must_use]")),
        "self-test [CF2]: an item with no `cfg` MUST classify as PRODUCTION"
    );
}

#[test]
fn readiness_raw_identifier_reference_is_normalized_and_reported() {
    // Fix 1 — raw-identifier (`r#`) normalisation. A production reference written
    // with raw-identifier syntax (`r#node_can_shell_raise(..)`) is the SAME
    // identifier under Rust's alternate lexical spelling (the seven readiness
    // symbols are all non-keyword names). `Ident::to_string` prints it WITH the
    // `r#` escape, so the bare-name filter would miss it without normalisation.

    // FIRE (RED against the un-normalised scan): a module-scope production fn that
    // CALLS the symbol with raw-identifier syntax IS reported under the bare name.
    let raw_call = r#"
        fn production_consumer(ctx: &C, node: SemanticNodeId) -> bool {
            r#node_can_shell_raise(ctx, node)
        }
    "#;
    assert!(
        readiness_symbols_referenced_in(raw_call).contains("node_can_shell_raise"),
        "self-test [Fix1]: a raw-spelled production call `r#node_can_shell_raise(..)` MUST be \
         reported under the bare name (raw-ident normalisation); got {:?}",
        readiness_symbols_referenced_in(raw_call)
    );

    // FIRE (RED): the same normalisation applies to a raw-spelled `use` import of
    // the symbol — `use ...::r#node_can_shell_raise;` records the bare name.
    let raw_use = r#"
        use crate::project_semantic_dispatch::raise::r#node_can_shell_raise;
    "#;
    assert!(
        readiness_symbols_referenced_in(raw_use).contains("node_can_shell_raise"),
        "self-test [Fix1]: a raw-spelled production `use ...::r#node_can_shell_raise;` MUST be \
         reported under the bare name; got {:?}",
        readiness_symbols_referenced_in(raw_use)
    );

    // FIRE (RED): a raw-spelled reference inside a MACRO token tree
    // (`some_macro!(... r#node_can_shell_raise(..) ...)`) is normalised on the
    // token-walker path too (`scan_macro_tokens_for_idents`), not only the
    // structured `visit_ident` path.
    let raw_macro = r#"
        fn production_consumer(ctx: &C, node: SemanticNodeId) -> bool {
            some_macro!(if r#node_can_shell_raise(ctx, node) { yes } else { no })
        }
    "#;
    assert!(
        readiness_symbols_referenced_in(raw_macro).contains("node_can_shell_raise"),
        "self-test [Fix1]: a raw-spelled reference inside a macro token tree MUST be reported under \
         the bare name (token-walker normalisation); got {:?}",
        readiness_symbols_referenced_in(raw_macro)
    );

    // RED-DISCRIMINATION (temp-sim of the PRE-fix scan): a `syn::Visit` that
    // records `id.to_string()` WITHOUT stripping `r#` records the ESCAPED spelling
    // `r#node_can_shell_raise`, so the bare-name filter (`contains("…")`) MISSES it
    // — proving the fixture is RED against the un-normalised scan, while the
    // production scan (normalised) reports the bare name.
    struct PreFixNoRawNormalise {
        idents: BTreeSet<String>,
    }
    impl<'ast> Visit<'ast> for PreFixNoRawNormalise {
        fn visit_ident(&mut self, id: &'ast proc_macro2::Ident) {
            self.idents.insert(id.to_string()); // no `r#` strip — the pre-fix shape
        }
    }
    let file = syn::parse_file(raw_call).expect("parse raw_call");
    let mut pre_fix = PreFixNoRawNormalise {
        idents: BTreeSet::new(),
    };
    pre_fix.visit_file(&file);
    assert!(
        pre_fix.idents.contains("r#node_can_shell_raise"),
        "self-test [Fix1] discrimination: the PRE-fix scan records the ESCAPED `r#…` spelling; got \
         {:?}",
        pre_fix.idents
    );
    assert!(
        !pre_fix.idents.contains("node_can_shell_raise"),
        "self-test [Fix1] discrimination: the PRE-fix scan does NOT record the bare name (so the \
         bare-name filter MISSES the raw-spelled reference) — proving the fixture is RED against the \
         un-normalised scan; got {:?}",
        pre_fix.idents
    );
}

#[test]
fn readiness_cfg_test_impl_item_skipped_on_whole_file_scan() {
    // Fix 2 — the EXACT-`#[cfg(test)]` impl-ITEM skip is hoisted UNCONDITIONAL, so
    // it applies on EVERY scan path including the whole-file scan
    // (`readiness_production_idents` / `readiness_symbols_referenced_in`), which
    // carries NO `impl_subtraction`. Before the hoist the cfg(test) skip was gated
    // on the impl-subtraction being active, so a `#[cfg(test)]` associated const /
    // type whose value/type references a readiness symbol was STILL descended on
    // the whole-file path — contradicting the documented "test-gated impl-items are
    // skipped" claim.

    // PASS (post-fix): a `#[cfg(test)]` associated CONST inside an impl whose value
    // references a readiness fn is test code and MUST NOT fire on the whole-file
    // scan. The impl head / other items reference no readiness token, so the ONLY
    // possible hit is the cfg(test) const's value — which the unconditional skip
    // now removes.
    let cfg_test_assoc_const = r#"
        struct Dispatch;
        impl Dispatch {
            #[cfg(test)]
            const PROBE: bool = node_can_shell_raise_marker;
        }
    "#;
    assert!(
        readiness_symbols_referenced_in(
            &cfg_test_assoc_const.replace("node_can_shell_raise_marker", "node_can_shell_raise")
        )
        .is_empty(),
        "self-test [Fix2]: a `#[cfg(test)]` associated const referencing a readiness symbol MUST NOT \
         fire on the WHOLE-FILE scan (the cfg(test) impl-item skip is unconditional); got {:?}",
        readiness_symbols_referenced_in(
            &cfg_test_assoc_const.replace("node_can_shell_raise_marker", "node_can_shell_raise")
        )
    );

    // PASS (post-fix): the same for a `#[cfg(test)]` associated TYPE whose aliased
    // type references the readiness artifact type.
    let cfg_test_assoc_type = r#"
        struct Dispatch;
        impl Dispatch {
            #[cfg(test)]
            type Probe = NodeBearingExpansion;
        }
    "#;
    assert!(
        readiness_symbols_referenced_in(cfg_test_assoc_type).is_empty(),
        "self-test [Fix2]: a `#[cfg(test)]` associated type referencing the readiness artifact type \
         MUST NOT fire on the WHOLE-FILE scan; got {:?}",
        readiness_symbols_referenced_in(cfg_test_assoc_type)
    );

    // RED-DISCRIMINATION (temp-sim of the PRE-fix gating): a `syn::Visit` whose
    // `visit_impl_item` skips a cfg(test) impl-item ONLY when an impl-subtraction
    // flag is active (here SIMULATED as inactive — the whole-file shape) descends
    // the cfg(test) const and RECORDS the readiness symbol — proving the fixture is
    // RED against the pre-hoist gating, while the production whole-file scan (with
    // the unconditional skip) does NOT.
    struct PreFixGatedCfgSkip {
        idents: BTreeSet<String>,
        // Simulates the whole-file scan: NO impl-subtraction, so the pre-fix code
        // skipped the cfg(test) check entirely.
        impl_subtraction_active: bool,
    }
    impl<'ast> Visit<'ast> for PreFixGatedCfgSkip {
        fn visit_impl_item(&mut self, item: &'ast syn::ImplItem) {
            if self.impl_subtraction_active {
                let attrs: &[syn::Attribute] = match item {
                    syn::ImplItem::Const(c) => &c.attrs,
                    syn::ImplItem::Type(t) => &t.attrs,
                    syn::ImplItem::Fn(f) => &f.attrs,
                    _ => &[],
                };
                if readiness_attrs_are_cfg_test(attrs) {
                    return;
                }
            }
            syn::visit::visit_impl_item(self, item);
        }
        fn visit_ident(&mut self, id: &'ast proc_macro2::Ident) {
            self.idents.insert(normalize_ident(id.to_string()));
        }
    }
    let probed =
        cfg_test_assoc_const.replace("node_can_shell_raise_marker", "node_can_shell_raise");
    let file = syn::parse_file(&probed).expect("parse cfg_test_assoc_const");
    let mut pre_fix = PreFixGatedCfgSkip {
        idents: BTreeSet::new(),
        impl_subtraction_active: false, // whole-file scan shape
    };
    pre_fix.visit_file(&file);
    assert!(
        pre_fix.idents.contains("node_can_shell_raise"),
        "self-test [Fix2] discrimination: the PRE-fix gated cfg-skip (inactive on the whole-file \
         shape) DESCENDS the cfg(test) const and records the readiness symbol — proving the fixture \
         is RED against the pre-hoist gating; got {:?}",
        pre_fix.idents
    );
}
