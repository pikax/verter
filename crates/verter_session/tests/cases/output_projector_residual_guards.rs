//! Output-materialization capability-fence residual guards (D1/D3).
//!
//! In safe production Rust OUTSIDE the audited output-materialization payload
//! vault, `OutputTypeExpr` and `MaterializedOutputTypeExpr` do not expose a
//! readable `TypeExpr` field: the inner `TypeExpr` lives in the deeply-private
//! `carrier::payload` vault in
//! `project_semantic_dispatch/output_materialization.rs`, so a capability-free
//! unwrap is unrepresentable by field access, auto-deref, an arbitrary trait
//! impl, or an inherent method, and the only production APIs returning
//! `TypeExpr` / `&TypeExpr` are the capability-gated `into_type_expr` /
//! `type_expr` accessors. The PRIMARY mint defense is the same shape: a hot /
//! session / Kind-B mint is a COMPILE error (`E0624`/`E0451`) because the
//! terminal-SINK capability constructor (`mint: pub(in <sink-module>)`, scoped
//! so its entire reachable production module tree is output-only — for the
//! projectors cap a dedicated `output_sink` submodule) is unreachable from any
//! non-sink module. The RESIDUAL TRUSTED SURFACE — the one the compiler cannot
//! itself police — is the inline payload vault + projector registration source
//! (and the by-name identity of which owner types are sinks), plus guard
//! deletion and unsafe code (the claim does NOT cover those unless the crate
//! forbids unsafe globally). Over that BOUNDED surface, the `syn` guards below
//! are DEFENSE-IN-DEPTH shaped as a CLOSED structural allowlist, not the
//! primary barrier.
//! These guards pin the EXACT shape of that trusted surface, each with the
//! mechanism that MATCHES it:
//!
//! 1. `residual_output_materialization_bridge_no_new_kind_b_references` —
//!    the interim Kind-B `legacy_semantic_type_expr_bridge` is a
//!    crate-visible (`pub(crate)`) non-sealed path. Rust visibility cannot
//!    distinguish the sanctioned historical Kind-B reference set from a NEW
//!    same-visibility hot reference, so this `syn` structural scanner pins
//!    the EXACT sanctioned reference set and bans any new one (until the
//!    Kind-B graph-native conversion — the end-state that retires the
//!    bridge — lands and it is deleted).
//!
//! 2. The fence-SHAPE guards — each pins one facet of the trusted vault /
//!    registration surface the compiler cannot express:
//!    - `sealed_module_is_private_not_pub_super` — pins the structural fact
//!      that makes the carrier-can't-name-`sealed` seal COMPILER-enforced:
//!      `mod sealed` inside `mod projector` is PRIVATE (no visibility modifier,
//!      NOT `pub(super)`), so a sibling `carrier`-side
//!      `impl projector::sealed::Sealed for HotCap` is `E0603` (module `sealed`
//!      is private). The PRIMARY barrier is now the compiler; the topology
//!      guard below is defense-in-depth. (A `pub(super)` would leak the marker
//!      to the parent `output_materialization` and the sibling `carrier`,
//!      re-opening the launder.)
//!    - `output_projector_owner_registration_inventory` — THREE checks: the
//!      sanctioned output-sink set is EXACTLY the eight explicit
//!      `impl OutputProjector for <Cap>` self-types AND the matching
//!      `impl sealed::Sealed for <Cap>` self-types are the SAME eight (with NO
//!      blanket/generic `impl<T> sealed::Sealed for T` — the residual a private
//!      `mod sealed` does not itself stop inside `mod projector`); registration
//!      is explicit `impl` items now, NOT a macro, so the real source impls are
//!      scanned — a hidden macro body can no longer mask an extra registration;
//!      AND the owner file's MODULE TOPOLOGY is EXACTLY the inline vault shape
//!      (`projector`, `projector::sealed`, `carrier`, `carrier::payload`, and
//!      nothing else), with item-position macro invocations / `include!` /
//!      unknown attribute macros BANNED, and `cfg_attr` parsed nested so only an
//!      inert applied attribute is allowed (`#[cfg_attr(unix, proc_macro)]` is
//!      caught) — `derive` is not broadly allowed (the owner derives nothing).
//!      The exact-topology + macro/include ban is what the old `Item::Mod`-by-
//!      kind count was blind to (a `macro_rules!`-emitted `mod` or an
//!      `include!`-injected module never appears as an `Item::Mod` in the
//!      parsed AST).
//!    - `output_carriers_have_no_inherent_typeexpr_escape_method` — a CLOSED
//!      ITEM/SIGNATURE ALLOWLIST over the carrier + payload vault: every
//!      production `fn` returning `TypeExpr` / `&TypeExpr` must be
//!      capability-gated (`P: OutputProjector`) or EXACTLY test-gated. This
//!      replaces the old finite name-blacklist (`into_inner` / `as_type_expr` /
//!      `as_inner`), which an unlisted name (`raw` / `leak` / `payload`) could
//!      evade — the allowlist keys on the SIGNATURE, not the name. It ALSO bans
//!      any `type X = …TypeExpr…` alias in the vault: the alias-launder
//!      (`type Inner = TypeExpr; fn alias_leak(&self) -> &Inner`) returns
//!      `&Inner` not `&TypeExpr`, so the return-type check alone would miss it —
//!      banning the alias at its declaration brings this guard to the field
//!      guard's robustness (symmetric on the alias trick).
//!    - `output_carrier_payload_fields_are_private` — EVERY field of the
//!      carrier/payload vault structs (`OutputPayload` / `OutputTypeExpr` /
//!      `MaterializedOutputTypeExpr`) stays private (reads `field.vis`, a
//!      structural fact), REGARDLESS of the spelled field type — so the
//!      `type Inner = TypeExpr; struct OutputPayload(pub Inner)` alias launder
//!      is caught too.
//!    - The carrier TRAIT escapes (`Deref<Target = TypeExpr>` /
//!      `AsRef<TypeExpr>` / `Borrow<TypeExpr>`) have a crate-wide
//!      `assert_not_impl_any!` ACCIDENTAL-REGRESSION CANARY in the sibling
//!      `src/project_semantic_dispatch/output_materialization_guards.rs` (the
//!      canary catches the COMMON accidental forms; completeness for the
//!      unbounded escape-trait surface comes from the payload vault, not the
//!      finite trait list). The out-of-crate visibility boundary is pinned by
//!      the trybuild fixture `output_projector_not_impl_outside_crate.rs`
//!      (`output_projector_non_owner_impl_is_compiler_sealed`).
//!
//! ```text
//! scanner_invariant: residual_output_materialization_bridge_no_new_kind_b_references
//! scanner_justification: legacy_semantic_type_expr_bridge is an explicit interim non-sealed Kind-B path; Rust visibility cannot pin the exact sanctioned historical reference set.
//! mechanism_ruling: structural-confinement-first — the sealed OutputProjector capability is the primary (compiler-enforced); this scanner is the bounded residual for the interim Kind-B bridge that the compiler cannot express. The bridge is retained only until the Kind-B graph-native conversion lands (tracked in docs/arch/parselower-design.md); no new Kind-B bridge sites.
//! hardening_rounds: 0
//! hardening_history: initial syn/token-tree structural detector; no prior spelling hardening.
//! ```
//!
//! Every `syn` `#[test]` guard here ships a paired self-test proving it
//! discriminates (fires on a synthetic violation, passes on the known-good
//! shape) per the Stub-Prevention contract.

use std::collections::BTreeMap;
use std::path::PathBuf;

use proc_macro2::TokenStream;
use quote::ToTokens;
use walkdir::WalkDir;

// Reuse the ONE rigorous parsed cfg classifier (EXACT canonical-shape
// recogniser) from the sibling guard module rather than forking a second,
// cruder substring matcher. `cfg_is_exactly_test_or_test_support` is the strict
// EXACT recogniser the carrier `_for_test` gate invariant needs. Divergent
// classifiers diverge; this keeps the fence-protecting guards on a single
// discriminating detector.
use super::handle_capable_consumer_guards::cfg_is_exactly_test_or_test_support;

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

/// Production `.rs` files under `crates/verter_session/src`, relative to the
/// crate root, test fixtures excluded.
fn production_src_files() -> Vec<(String, String)> {
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
        if is_test_file(&rel) {
            continue;
        }
        let src = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        out.push((rel, src));
    }
    out.sort();
    out
}

// ===========================================================================
// (1) Kind-B bridge no-new-reference scanner.
// ===========================================================================

const BRIDGE_IDENT: &str = "legacy_semantic_type_expr_bridge";

/// The KIND of a reference to the bridge identifier, so the allowlist can
/// pin definitions, method calls, path calls, and bare references
/// separately (a bare method-item reference / alias is the laundering
/// escape the brief calls out: `let f = …bridge; f(…)`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum RefKind {
    /// `fn legacy_semantic_type_expr_bridge(…)` definition.
    Definition,
    /// `recv.legacy_semantic_type_expr_bridge(…)` method call.
    MethodCall,
    /// `Path::legacy_semantic_type_expr_bridge(…)` path call, OR a bare
    /// path reference (`use …`, fn-item reference, const fn pointer). Any
    /// NON-method-call appearance of the identifier as a path/ident token.
    PathOrBareRef,
    /// The identifier appears as a token inside a macro invocation's body.
    MacroToken,
}

/// One sanctioned reference site: `(file, fn-name-or-context, kind)`. The
/// occurrence count is carried in the collected map's value.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct BridgeSite {
    file: String,
    /// The enclosing item name (the `fn` the reference sits in), or
    /// `<definition>` for the definition site, or `<file-scope>` for a
    /// file-scope reference (e.g. a `use`).
    context: String,
    kind: RefKind,
}

/// `syn` visitor that records every reference to [`BRIDGE_IDENT`] —
/// COMMENT/STRING-BLIND by construction (a `syn` AST / token stream carries
/// no comment tokens, and a string/char literal is a `Lit`, never an
/// `Ident`). It descends inline modules and into macro token trees.
struct BridgeRefCollector {
    file: String,
    /// Current enclosing-fn name stack (for the `context` anchor).
    fn_stack: Vec<String>,
    sites: Vec<BridgeSite>,
}

impl BridgeRefCollector {
    fn context(&self) -> String {
        self.fn_stack
            .last()
            .cloned()
            .unwrap_or_else(|| "<file-scope>".to_string())
    }

    /// Scan a macro token stream for the bridge identifier, ignoring
    /// literals (a raw/byte/char/string literal is a `Literal` token, never
    /// an `Ident`, so bridge text inside a string is NOT a hit).
    fn scan_macro_tokens(&mut self, tokens: &TokenStream) {
        for tt in tokens.clone() {
            match tt {
                proc_macro2::TokenTree::Ident(id) => {
                    if id == BRIDGE_IDENT {
                        self.sites.push(BridgeSite {
                            file: self.file.clone(),
                            context: self.context(),
                            kind: RefKind::MacroToken,
                        });
                    }
                }
                proc_macro2::TokenTree::Group(g) => self.scan_macro_tokens(&g.stream()),
                _ => {}
            }
        }
    }

    /// Does this path's final segment name the bridge identifier?
    fn path_is_bridge(path: &syn::Path) -> bool {
        path.segments
            .last()
            .map(|s| s.ident == BRIDGE_IDENT)
            .unwrap_or(false)
    }
}

impl<'ast> syn::visit::Visit<'ast> for BridgeRefCollector {
    fn visit_impl_item_fn(&mut self, f: &'ast syn::ImplItemFn) {
        let is_def = f.sig.ident == BRIDGE_IDENT;
        if is_def {
            self.sites.push(BridgeSite {
                file: self.file.clone(),
                context: "<definition>".to_string(),
                kind: RefKind::Definition,
            });
        }
        self.fn_stack.push(f.sig.ident.to_string());
        syn::visit::visit_impl_item_fn(self, f);
        self.fn_stack.pop();
    }

    fn visit_item_fn(&mut self, f: &'ast syn::ItemFn) {
        let is_def = f.sig.ident == BRIDGE_IDENT;
        if is_def {
            self.sites.push(BridgeSite {
                file: self.file.clone(),
                context: "<definition>".to_string(),
                kind: RefKind::Definition,
            });
        }
        self.fn_stack.push(f.sig.ident.to_string());
        syn::visit::visit_item_fn(self, f);
        self.fn_stack.pop();
    }

    fn visit_expr_method_call(&mut self, mc: &'ast syn::ExprMethodCall) {
        if mc.method == BRIDGE_IDENT {
            self.sites.push(BridgeSite {
                file: self.file.clone(),
                context: self.context(),
                kind: RefKind::MethodCall,
            });
        }
        syn::visit::visit_expr_method_call(self, mc);
    }

    fn visit_expr_path(&mut self, p: &'ast syn::ExprPath) {
        if Self::path_is_bridge(&p.path) {
            self.sites.push(BridgeSite {
                file: self.file.clone(),
                context: self.context(),
                kind: RefKind::PathOrBareRef,
            });
        }
        syn::visit::visit_expr_path(self, p);
    }

    fn visit_use_path(&mut self, u: &'ast syn::UsePath) {
        // A `use …::legacy_semantic_type_expr_bridge` (import / alias /
        // re-export) names the identifier as the LAST tree segment.
        if u.ident == BRIDGE_IDENT {
            self.sites.push(BridgeSite {
                file: self.file.clone(),
                context: "<use-import>".to_string(),
                kind: RefKind::PathOrBareRef,
            });
        }
        syn::visit::visit_use_path(self, u);
    }

    fn visit_use_name(&mut self, u: &'ast syn::UseName) {
        if u.ident == BRIDGE_IDENT {
            self.sites.push(BridgeSite {
                file: self.file.clone(),
                context: "<use-import>".to_string(),
                kind: RefKind::PathOrBareRef,
            });
        }
        syn::visit::visit_use_name(self, u);
    }

    fn visit_use_rename(&mut self, u: &'ast syn::UseRename) {
        // `use …bridge as alias;` — the source ident is the laundering
        // escape (aliasing the call set). Record the SOURCE ident.
        if u.ident == BRIDGE_IDENT {
            self.sites.push(BridgeSite {
                file: self.file.clone(),
                context: "<use-rename>".to_string(),
                kind: RefKind::PathOrBareRef,
            });
        }
        syn::visit::visit_use_rename(self, u);
    }

    fn visit_macro(&mut self, m: &'ast syn::Macro) {
        self.scan_macro_tokens(&m.tokens);
        syn::visit::visit_macro(self, m);
    }
}

/// Collect every production reference to the bridge identifier, grouped by
/// `(BridgeSite -> occurrence count)`.
fn collect_bridge_references() -> BTreeMap<BridgeSite, usize> {
    let mut counts: BTreeMap<BridgeSite, usize> = BTreeMap::new();
    for (rel, src) in production_src_files() {
        let file = match syn::parse_file(&src) {
            Ok(f) => f,
            Err(_) => continue, // non-parseable file (e.g. generated): skip
        };
        let mut collector = BridgeRefCollector {
            file: rel.clone(),
            fn_stack: Vec::new(),
            sites: Vec::new(),
        };
        syn::visit::Visit::visit_file(&mut collector, &file);
        for site in collector.sites {
            *counts.entry(site).or_insert(0) += 1;
        }
    }
    counts
}

/// The EXACT sanctioned reference set for `legacy_semantic_type_expr_bridge`
/// (the sanctioned interim Kind-B path, retained only until the Kind-B
/// graph-native conversion retires it). Each
/// row: `(file, context, kind, expected_count)`. Any reference NOT in this
/// set — or a count drift on a pinned row — fails the guard (a NEW Kind-B
/// bridge site, the D3 residual the compiler cannot express).
fn sanctioned_bridge_sites() -> BTreeMap<BridgeSite, usize> {
    let rows: &[(&str, &str, RefKind, usize)] = &[
        // The single definition (the still-private-primitive delegator).
        (
            "src/project_semantic_dispatch/raise.rs",
            "<definition>",
            RefKind::Definition,
            1,
        ),
        // The `execute_to_type_expr` route-fixpoint raise.
        (
            "src/project_semantic_dispatch/mod.rs",
            "execute_to_type_expr",
            RefKind::MethodCall,
            1,
        ),
        // The slot-binding terminal-id raise.
        (
            "src/project_semantic_dispatch/mod.rs",
            "project_slot_binding_member_with_terminal_id",
            RefKind::MethodCall,
            1,
        ),
        // `instantiate_local_generic_ref_via_dispatch` (threaded-route helper).
        (
            "src/meta_resolve/dispatch_helpers.rs",
            "instantiate_local_generic_ref_via_dispatch",
            RefKind::MethodCall,
            1,
        ),
        // The `fast_to_expansion` expansion path (the enclosing host method
        // is `compute_evaluated_types_from_owner_context_with_ctx`) — two
        // raise sites (the model-payload base and the produced node id).
        (
            "src/host_manage/eval_env.rs",
            "compute_evaluated_types_from_owner_context_with_ctx",
            RefKind::MethodCall,
            2,
        ),
    ];
    rows.iter()
        .map(|(file, ctx, kind, count)| {
            (
                BridgeSite {
                    file: (*file).to_string(),
                    context: (*ctx).to_string(),
                    kind: *kind,
                },
                *count,
            )
        })
        .collect()
}

#[test]
fn residual_output_materialization_bridge_no_new_kind_b_references() {
    let observed = collect_bridge_references();
    let sanctioned = sanctioned_bridge_sites();

    // (a) No NEW / unsanctioned reference site (or a count drift): every
    // observed site must be in the sanctioned set with the SAME count.
    let mut unexpected: Vec<String> = Vec::new();
    for (site, count) in &observed {
        match sanctioned.get(site) {
            Some(expected) if expected == count => {}
            Some(expected) => unexpected.push(format!(
                "{}::{} [{:?}] — count drift: observed {count}, sanctioned {expected}",
                site.file, site.context, site.kind
            )),
            None => unexpected.push(format!(
                "{}::{} [{:?}] — NEW unsanctioned bridge reference (x{count})",
                site.file, site.context, site.kind
            )),
        }
    }
    assert!(
        unexpected.is_empty(),
        "D3 residual: `{BRIDGE_IDENT}` is the interim Kind-B reverse-raise path (retained only \
         until the Kind-B graph-native conversion retires it); NO new reference may be \
         introduced. Route new output sinks through the \
         sealed `OutputProjector` capability instead, and do the Kind-B graph-native conversion \
         as the Kind-B graph-native conversion. Offending references:\n{}",
        unexpected.join("\n")
    );

    // (b) Anti-vacuity: every sanctioned site must still EXIST (a removed
    // bridge site means the Kind-B graph-native conversion may have already run, or a refactor silently
    // dropped a Kind-B path — either way this pin is stale and must be
    // re-baselined deliberately, not left asserting a phantom set).
    let mut missing: Vec<String> = Vec::new();
    for (site, expected) in &sanctioned {
        match observed.get(site) {
            Some(count) if count == expected => {}
            Some(count) => missing.push(format!(
                "{}::{} [{:?}] — count drift: sanctioned {expected}, observed {count}",
                site.file, site.context, site.kind
            )),
            None => missing.push(format!(
                "{}::{} [{:?}] — sanctioned site MISSING (x{expected})",
                site.file, site.context, site.kind
            )),
        }
    }
    assert!(
        missing.is_empty(),
        "D3 residual ANTI-VACUITY: a sanctioned `{BRIDGE_IDENT}` reference is gone. If the \
         Kind-B graph-native conversion \
         removed the Kind-B path, delete the corresponding row from `sanctioned_bridge_sites()` \
         (and, when the set is empty, delete the bridge + this guard). Do NOT leave a phantom \
         pin. Missing:\n{}",
        missing.join("\n")
    );
}

#[test]
fn bridge_scanner_self_test_discriminates() {
    // The collector must (a) be comment/string-blind, (b) catch a real
    // method call, (c) catch a path-call / bare fn-item reference, (d)
    // catch a `use` alias (the laundering escape), and (e) catch a macro
    // token. We drive it on synthetic sources directly.
    fn scan(src: &str) -> Vec<BridgeSite> {
        let file = syn::parse_file(src).expect("parse synthetic source");
        let mut c = BridgeRefCollector {
            file: "<synthetic>".to_string(),
            fn_stack: Vec::new(),
            sites: Vec::new(),
        };
        syn::visit::Visit::visit_file(&mut c, &file);
        c.sites
    }

    // (a) BLIND: bridge text in a line comment, a block comment, and a
    // string/char literal MUST NOT be a hit. (Outer delimiter is `r##` so
    // the INNER `r#"…"#` raw-string literal in the fixture does not close
    // it.)
    let blind = r##"
        fn unrelated() {
            // legacy_semantic_type_expr_bridge in a line comment
            /* legacy_semantic_type_expr_bridge in a block comment */
            let _s = "legacy_semantic_type_expr_bridge in a string";
            let _r = r#"legacy_semantic_type_expr_bridge in a raw string"#;
            let _c = 'x';
            println!("legacy_semantic_type_expr_bridge in a format string");
        }
    "##;
    assert!(
        scan(blind).is_empty(),
        "self-test: bridge text in comments / string / raw-string literals MUST NOT be a hit \
         (comment/string-blind by construction)"
    );

    // (b) RED: a real method call is a hit.
    let method = r#"
        fn caller(d: &D) { let _ = d.legacy_semantic_type_expr_bridge(node); }
    "#;
    let m = scan(method);
    assert!(
        m.iter().any(|s| s.kind == RefKind::MethodCall && s.context == "caller"),
        "self-test: a real `.legacy_semantic_type_expr_bridge(…)` method call MUST be a hit; got {m:?}"
    );

    // (c) RED: a bare fn-item reference (no immediate call) is a hit — the
    // `let f = Path::…bridge; f(…)` laundering escape.
    let bare = r#"
        fn caller(d: &D) { let f = D::legacy_semantic_type_expr_bridge; f(d, node); }
    "#;
    let b = scan(bare);
    assert!(
        b.iter().any(|s| s.kind == RefKind::PathOrBareRef),
        "self-test: a bare fn-item reference (no immediate call) MUST be a hit — it launders the \
         call set; got {b:?}"
    );

    // (d) RED: a `use … as alias` import is a hit (aliasing the call set).
    let aliased = r#"
        use crate::project_semantic_dispatch::raise::legacy_semantic_type_expr_bridge as sneaky;
        fn caller() { let _ = sneaky; }
    "#;
    let a = scan(aliased);
    assert!(
        a.iter()
            .any(|s| s.kind == RefKind::PathOrBareRef && s.context == "<use-rename>"),
        "self-test: a `use …bridge as alias` import MUST be a hit; got {a:?}"
    );

    // (e) RED: the identifier inside a macro token tree is a hit.
    let in_macro = r#"
        fn caller(d: &D) { some_macro!(d.legacy_semantic_type_expr_bridge(node)); }
    "#;
    let mac = scan(in_macro);
    assert!(
        mac.iter().any(|s| s.kind == RefKind::MacroToken),
        "self-test: the bridge identifier inside a macro token tree MUST be a hit; got {mac:?}"
    );

    // NEGATIVE: a DIFFERENT identifier sharing a prefix MUST NOT be a hit.
    let prefixed = r#"
        fn caller(d: &D) { let _ = d.legacy_semantic_type_expr_bridge_other(node); }
    "#;
    assert!(
        scan(prefixed).is_empty(),
        "self-test: `legacy_semantic_type_expr_bridge_other` is a DIFFERENT identifier and MUST \
         NOT be a hit"
    );
}

#[test]
fn bridge_sanctioned_set_is_exactly_observed() {
    // Cross-check: the live observed set EQUALS the sanctioned set exactly.
    // This is the discriminating positive proof that the pin reflects the
    // real tree (a hand-edited allowlist that drifted from reality fails
    // here even if both directional checks above somehow passed).
    let observed = collect_bridge_references();
    let sanctioned = sanctioned_bridge_sites();
    assert_eq!(
        observed, sanctioned,
        "the observed `{BRIDGE_IDENT}` reference set must EQUAL the sanctioned set exactly"
    );
}

// scanner_invariant: kind_b_raise_then_decide_entrypoints_pinned
// scanner_justification: the `sanctioned_bridge_sites()` scanner pins the bridge REFERENCES (the `legacy_semantic_type_expr_bridge` calls INSIDE the Kind-B methods), but NOT new CALLERS of the raw-node entrypoints `execute_to_type_expr` / `project_slot_binding_member_with_terminal_id`, nor their VISIBILITY — a new crate-internal caller (or a widening to `pub`) would broaden the Kind-B raw-node→TypeExpr residual WITHOUT tripping the bridge-reference scanner. This entrypoint inventory pins the two methods' visibility (`pub(crate)`, fail on widening) + their EXACT production caller set across EVERY caller spelling (method call, UFCS / path call, bare fn-item reference, `use` import / rename, macro token — the same forms the bridge-reference scanner already covers). It is a residual the compiler cannot express: the Kind-B residual is DEFERRED to the Kind-B graph-native conversion (docs/arch/parselower-design.md `Stage8-A4-kind-b-graph-native-conversion`), so until it lands these entrypoints must not grow new callers. CLAIM SCOPE (honest): this pin enforces a caller-SET + visibility pin over the two named entrypoints — it is NOT a proof that no NEW Kind-B raise-then-decide ROUTE could be introduced (a brand-new raw-node→TypeExpr entrypoint method would need its own row). The structurally-PREFERRED mechanism (a sealed zero-sized permit type per sanctioned caller, or deleting the bridge) is part of the 8-A4 Kind-B graph-native conversion; this scanner is the second-best interim pin while the residual is deferred.
// mechanism_ruling: 8a3-guard-mechanism-consult-2026-06-24 (Kind-B caller pin kept-as-scanner; UFCS / function-item / use-alias coverage is bounded spelling-hardening)
// hardening_rounds: 1
// hardening_history: adoption — added alongside the bridge-reference scanner so the raw-node ENTRYPOINT surface (callers + visibility), not only the bridge references, is pinned for the deferred Kind-B residual. The single bounded spelling-hardening increment (hardening_rounds now one) extended the caller counter from `.method(…)` only to the full bridge-scanner caller-form set (UFCS / path call `Type::method(…)`, bare fn-item reference, `use` import / rename, macro token), so a UFCS-spelled caller can no longer broaden the deferred residual untripped. Bounded spelling-hardening (the compiler-rework / sealed-permit close is the deferred Kind-B graph-native disposition).

/// One Kind-B raw-node→`TypeExpr` entrypoint to pin: a `ProjectSemanticDispatch`
/// method that takes a caller-supplied raw subject (a `SemanticNodeId` base or
/// any `SemanticQueryKey`), raises it through the interim
/// `legacy_semantic_type_expr_bridge`, and SEMANTICALLY DECIDES on the raised
/// `TypeExpr` — the documented Kind-B residual deferred to 8-A4. The fix keeps
/// these from silently growing new callers (in ANY spelling) / widening
/// visibility while deferred. It does NOT prove no NEW entrypoint could appear —
/// a fresh raw-node→`TypeExpr` route would need its own row here.
struct KindBEntrypoint {
    /// The method name (matched on a `.method(` call site and the `fn` def).
    method: &'static str,
    /// The sanctioned production caller set: `file (crate-relative)` -> exact
    /// method-call-site count. A NEW caller file, or a count drift, fails.
    sanctioned_callers: &'static [(&'static str, usize)],
}

/// The pinned Kind-B entrypoints + their CURRENT production caller set
/// (verified against the live tree). The id-less `project_slot_binding_member`
/// is DELETED (was dead — no production caller), so only the terminal-id variant
/// remains; its sole caller is `eval_env.rs::fast_to_expansion`.
fn kind_b_entrypoints() -> &'static [KindBEntrypoint] {
    &[
        KindBEntrypoint {
            method: "execute_to_type_expr",
            sanctioned_callers: &[("src/meta_resolve/dispatch_helpers.rs", 3)],
        },
        KindBEntrypoint {
            method: "project_slot_binding_member_with_terminal_id",
            sanctioned_callers: &[("src/host_manage/eval_env.rs", 1)],
        },
    ]
}

/// The crate-relative file that DEFINES the Kind-B entrypoints (their `fn` items
/// live on the `ProjectSemanticDispatch` impl here). Visibility is read from the
/// `fn` definitions in this file; call sites in this file are the definition's
/// own surface and are NOT counted as external callers (today there are none —
/// the id-less wrapper that called the terminal-id variant was deleted).
const KIND_B_DEF_REL: &str = "src/project_semantic_dispatch/mod.rs";

/// A `syn` visitor counting EVERY reference to a Kind-B entrypoint `method` in a
/// file, matching the rigor of [`BridgeRefCollector`] so a caller cannot evade
/// the pin by spelling the call differently. COMMENT/STRING-BLIND by
/// construction (a `syn` AST / token stream carries no comment tokens; a
/// string/char literal is a `Lit`, never an `Ident`). It catches, by matching
/// the method as the FINAL path segment:
///   - a `.method(…)` method call (`ExprMethodCall`);
///   - a UFCS / path call `Type::method(…)` / `Self::method(…)` — represented as
///     an `ExprCall` whose callee is an `ExprPath` ending in `method`, caught
///     via the `ExprPath` visit (the callee is a single path visited once, so it
///     is NOT double-counted with the `ExprCall`);
///   - a bare function-item reference `let f = Type::method;` (`ExprPath`, no
///     immediate call — the laundering escape `f(d, k)`);
///   - a `use …::method` / `use …::method as alias` import (the alias-laundering
///     escape, naming the method as the last `use`-tree segment);
///   - the method identifier inside a macro token tree.
struct KindBCallerCollector {
    method: String,
    count: usize,
}

impl KindBCallerCollector {
    /// Does this path's FINAL segment name the pinned method?
    fn path_is_method(&self, path: &syn::Path) -> bool {
        path.segments
            .last()
            .map(|s| s.ident == self.method.as_str())
            .unwrap_or(false)
    }

    fn scan_macro_tokens(&mut self, tokens: &TokenStream) {
        for tt in tokens.clone() {
            match tt {
                proc_macro2::TokenTree::Ident(id) => {
                    if id == self.method.as_str() {
                        self.count += 1;
                    }
                }
                proc_macro2::TokenTree::Group(g) => self.scan_macro_tokens(&g.stream()),
                _ => {}
            }
        }
    }
}

impl<'ast> syn::visit::Visit<'ast> for KindBCallerCollector {
    fn visit_expr_method_call(&mut self, mc: &'ast syn::ExprMethodCall) {
        if mc.method == self.method.as_str() {
            self.count += 1;
        }
        syn::visit::visit_expr_method_call(self, mc);
    }

    fn visit_expr_path(&mut self, p: &'ast syn::ExprPath) {
        // A UFCS call `T::method(args)` is an `ExprCall` whose `func` is exactly
        // this `ExprPath`; a bare fn-item reference `let f = T::method;` is also
        // an `ExprPath`. Both land here (visited once), so both forms count
        // without double-counting against `visit_expr_call`.
        if self.path_is_method(&p.path) {
            self.count += 1;
        }
        syn::visit::visit_expr_path(self, p);
    }

    fn visit_use_path(&mut self, u: &'ast syn::UsePath) {
        if u.ident == self.method.as_str() {
            self.count += 1;
        }
        syn::visit::visit_use_path(self, u);
    }

    fn visit_use_name(&mut self, u: &'ast syn::UseName) {
        if u.ident == self.method.as_str() {
            self.count += 1;
        }
        syn::visit::visit_use_name(self, u);
    }

    fn visit_use_rename(&mut self, u: &'ast syn::UseRename) {
        // `use …method as alias;` — the SOURCE ident is the laundering escape.
        if u.ident == self.method.as_str() {
            self.count += 1;
        }
        syn::visit::visit_use_rename(self, u);
    }

    fn visit_macro(&mut self, m: &'ast syn::Macro) {
        self.scan_macro_tokens(&m.tokens);
        syn::visit::visit_macro(self, m);
    }
}

/// Collect, per production file, the COUNT of references to `method` (every
/// caller form above), EXCLUDING the definition file (its own internal use is
/// not an external caller). `syn`-based, so comments / strings never count.
fn kind_b_caller_counts(method: &str) -> BTreeMap<String, usize> {
    let mut out = BTreeMap::new();
    for (rel, src) in production_src_files() {
        if rel == KIND_B_DEF_REL {
            continue; // the definition file's own surface
        }
        let Ok(file) = syn::parse_file(&src) else {
            continue;
        };
        let mut c = KindBCallerCollector {
            method: method.to_string(),
            count: 0,
        };
        syn::visit::Visit::visit_file(&mut c, &file);
        if c.count > 0 {
            out.insert(rel, c.count);
        }
    }
    out
}

/// The visibility of the `fn <method>` definition in the [`KIND_B_DEF_REL`]
/// file, as a normalized string (`pub`, `pub(crate)`, `pub(super)`,
/// `pub(in …)`, or `private`). `None` if the fn is not found (anti-vacuity).
fn kind_b_entrypoint_visibility(file: &syn::File, method: &str) -> Option<String> {
    fn vis_str(vis: &syn::Visibility) -> String {
        match vis {
            syn::Visibility::Public(_) => "pub".to_string(),
            syn::Visibility::Inherited => "private".to_string(),
            syn::Visibility::Restricted(r) => {
                if r.path.is_ident("crate") {
                    "pub(crate)".to_string()
                } else if r.path.is_ident("super") {
                    "pub(super)".to_string()
                } else if r.path.is_ident("self") {
                    "private".to_string()
                } else {
                    format!("pub(in {})", quote::quote!(#r).to_string().replace(' ', ""))
                }
            }
        }
    }
    struct V {
        method: String,
        vis: Option<String>,
    }
    impl<'ast> syn::visit::Visit<'ast> for V {
        fn visit_impl_item_fn(&mut self, f: &'ast syn::ImplItemFn) {
            if f.sig.ident == self.method.as_str() {
                self.vis = Some(vis_str(&f.vis));
            }
            syn::visit::visit_impl_item_fn(self, f);
        }
    }
    let mut v = V {
        method: method.to_string(),
        vis: None,
    };
    syn::visit::Visit::visit_file(&mut v, file);
    v.vis
}

#[test]
fn kind_b_raise_then_decide_entrypoints_pinned() {
    // The Kind-B raw-node→TypeExpr residual is DEFERRED to the Kind-B
    // graph-native conversion (docs/arch/parselower-design.md). Until it lands,
    // these entrypoints must stay `pub(crate)` and must not grow new production
    // callers in ANY spelling (method call / UFCS path call / bare fn-item ref /
    // `use` import-rename / macro token) — else the residual broadens untripped
    // by the bridge-reference scanner. This pins the caller SET + visibility of
    // the two named entrypoints; it is NOT a proof that no NEW Kind-B
    // raise-then-decide route could be introduced (the sealed-permit / bridge
    // deletion is the 8-A4 close).
    let def_file = syn::parse_file(&read_rel(KIND_B_DEF_REL))
        .unwrap_or_else(|e| panic!("parse {KIND_B_DEF_REL}: {e}"));

    let mut violations: Vec<String> = Vec::new();
    for ep in kind_b_entrypoints() {
        // (a) Visibility pin: must be EXACTLY `pub(crate)` (a widening to `pub`
        // exports the raw-node entrypoint to off-crate consumers).
        match kind_b_entrypoint_visibility(&def_file, ep.method) {
            Some(vis) if vis == "pub(crate)" => {}
            Some(vis) => violations.push(format!(
                "Kind-B entrypoint `{}` visibility is `{vis}` — MUST stay `pub(crate)` (a widening \
                 exports the raw-node→TypeExpr residual; close it via the Kind-B graph-native \
                 conversion instead of widening)",
                ep.method
            )),
            None => violations.push(format!(
                "anti-vacuity: Kind-B entrypoint `{}` `fn` definition NOT FOUND in {KIND_B_DEF_REL} \
                 — it was renamed / moved without updating this pin",
                ep.method
            )),
        }

        // (b) Caller pin: the production method-call-site set must EQUAL the
        // sanctioned set (a NEW caller file, or a count drift, fails).
        let observed = kind_b_caller_counts(ep.method);
        let sanctioned: BTreeMap<String, usize> = ep
            .sanctioned_callers
            .iter()
            .map(|(f, c)| ((*f).to_string(), *c))
            .collect();
        if observed != sanctioned {
            violations.push(format!(
                "Kind-B entrypoint `{}` production caller set drifted — observed {observed:?}, \
                 sanctioned {sanctioned:?}. A NEW caller broadens the deferred Kind-B residual; \
                 route it through the shared resolver / publication-authority chain, or land the \
                 Kind-B graph-native conversion. If a caller was legitimately removed, re-baseline \
                 `kind_b_entrypoints()` deliberately.",
                ep.method
            ));
        }
    }
    assert!(
        violations.is_empty(),
        "Kind-B raw-node→`TypeExpr` entrypoint pin violation(s):\n{}",
        violations.join("\n")
    );
}

#[test]
fn kind_b_entrypoint_pin_self_test_discriminates() {
    // (a) Visibility reader discriminates pub(crate) vs a widening to pub.
    let crate_vis = r#"
        impl D {
            pub(crate) fn execute_to_type_expr(&self, k: &K) -> R { todo!() }
        }
    "#;
    let f = syn::parse_file(crate_vis).expect("parse crate-vis");
    assert_eq!(
        kind_b_entrypoint_visibility(&f, "execute_to_type_expr").as_deref(),
        Some("pub(crate)"),
        "self-test: a `pub(crate)` entrypoint reads as `pub(crate)`"
    );
    let widened = r#"
        impl D {
            pub fn execute_to_type_expr(&self, k: &K) -> R { todo!() }
        }
    "#;
    let f = syn::parse_file(widened).expect("parse widened");
    assert_eq!(
        kind_b_entrypoint_visibility(&f, "execute_to_type_expr").as_deref(),
        Some("pub"),
        "self-test: a WIDENED `pub` entrypoint reads as `pub` (≠ pub(crate)), so the guard fires"
    );

    // (b) Caller counter discriminates a call from a same-named non-call and is
    // comment/string-blind. Drives the REAL [`KindBCallerCollector`] (the
    // production counter) so the self-test exercises the exact code the guard
    // runs — it must match the rigor of the bridge-reference scanner (method
    // call + UFCS / path call + bare fn-item ref + `use` import / rename + macro
    // token), not just `.method(…)`.
    let scan = |src: &str| -> usize {
        let file = syn::parse_file(src).expect("parse caller-src");
        let mut c = KindBCallerCollector {
            method: "execute_to_type_expr".to_string(),
            count: 0,
        };
        syn::visit::Visit::visit_file(&mut c, &file);
        c.count
    };
    // Two real method calls + a comment + a string mentioning the name.
    let with_calls = r##"
        fn caller(d: &D) {
            // execute_to_type_expr in a comment
            let _s = "execute_to_type_expr in a string";
            let _a = d.execute_to_type_expr(&k1);
            let _b = d.execute_to_type_expr(&k2);
        }
    "##;
    assert_eq!(
        scan(with_calls),
        2,
        "self-test: exactly the TWO real method calls count (comment + string blind)"
    );
    // A NEW method-call caller (count 1) differs from a sanctioned
    // ZERO-for-that-file → the set-equality check in the guard fires.
    let new_caller = r#"
        fn other(d: &D) { let _ = d.execute_to_type_expr(&k); }
    "#;
    assert_eq!(
        scan(new_caller),
        1,
        "self-test: a new caller site is counted (a fresh file with count 1 breaks set-equality)"
    );
    // RED — UFCS / path call: `ProjectSemanticDispatch::execute_to_type_expr(&d, &k)`
    // is an `ExprCall` whose callee path ends in the method. The old
    // method-call-only counter MISSED this (returned 0); the rigor-matched
    // collector MUST count it as a caller (matched as the final path segment).
    let ufcs_call = r#"
        fn other(d: &D) { let _ = ProjectSemanticDispatch::execute_to_type_expr(&d, &k); }
    "#;
    assert_eq!(
        scan(ufcs_call),
        1,
        "self-test: a UFCS / path call `Type::execute_to_type_expr(…)` MUST be counted (the \
         method-call-only counter missed it); counted ONCE (callee path visited once, no \
         double-count vs the enclosing call)"
    );
    // RED — bare function-item reference: `let f = Type::execute_to_type_expr; f(&d, &k);`
    // launders the call set behind an indirection. MUST be counted.
    let bare_ref = r#"
        fn other(d: &D) { let f = ProjectSemanticDispatch::execute_to_type_expr; f(&d, &k); }
    "#;
    assert_eq!(
        scan(bare_ref),
        1,
        "self-test: a bare fn-item reference `let f = Type::execute_to_type_expr;` MUST be counted \
         (it launders the caller set)"
    );
    // RED — `use … as alias` import names the method as the last `use`-tree
    // segment (the alias-laundering escape). MUST be counted.
    let use_rename = r#"
        use crate::project_semantic_dispatch::ProjectSemanticDispatch::execute_to_type_expr as sneaky;
        fn other(d: &D) { let _ = sneaky(&d, &k); }
    "#;
    assert!(
        scan(use_rename) >= 1,
        "self-test: a `use …execute_to_type_expr as alias` import MUST be counted (alias laundering)"
    );
    // RED — the method identifier inside a macro token tree.
    let in_macro = r#"
        fn other(d: &D) { some_macro!(d.execute_to_type_expr(&k)); }
    "#;
    assert!(
        scan(in_macro) >= 1,
        "self-test: the entrypoint identifier inside a macro token tree MUST be counted"
    );
    // NEGATIVE — a DIFFERENT identifier sharing a prefix MUST NOT count.
    let prefixed = r#"
        fn other(d: &D) { let _ = d.execute_to_type_expr_other(&k); }
    "#;
    assert_eq!(
        scan(prefixed),
        0,
        "self-test: `execute_to_type_expr_other` is a DIFFERENT identifier and MUST NOT count"
    );
}

// ===========================================================================
// (2) OutputProjector owner-file fence-shape guards: the sanctioned sink-set
// registration inventory + EXACT module-topology confinement, the carrier
// item/signature accessor allowlist, and the carrier/payload field privacy. The
// carrier TRAIT escapes have an accidental-regression CANARY in the sibling
// `src/project_semantic_dispatch/output_materialization_guards.rs`; the COMPLETE
// safe-Rust mechanism is the payload vault (no readable `TypeExpr` field outside
// `carrier::payload`).
// ===========================================================================

const OWNER_REL: &str = "src/project_semantic_dispatch/output_materialization.rs";

/// The FULL `::`-joined capability self-type paths the EXPLICIT
/// `impl OutputProjector for <Cap>` registration is allowed for — the EIGHT
/// true-output-sink capabilities (one per exact output-sink module that
/// legitimately projects). Compared as a MULTISET against the observed full
/// self-type paths (NOT deduped last-idents — so two impls for `a::b::Cap` and
/// `c::d::Cap` are distinct entries; the duplicate-last-ident gap is closed).
/// The `#[cfg(test)]`-gated `TestOutputCap` registers through a test-gated impl,
/// so it is excluded from this production set by the test-gate check. A new
/// production `impl OutputProjector for <other>` widens the laundering surface
/// and fails [`output_projector_owner_registration_inventory`].
///
/// This is a by-PATH IDENTITY allowlist of the sanctioned output sinks Rust
/// cannot express, hence the guard-local Structural-Confinement record:
///
/// ```text
/// scanner_invariant: output_projector_sink_set_is_exactly_sanctioned
/// scanner_justification: Rust cannot express "these specific types are the sanctioned output sinks"; the sealed trait stops out-of-owner impls but does not encode WHICH owner-named types may be sinks.
/// mechanism_ruling: structural-confinement-first — the sealed OutputProjector capability + the payload vault are the compiler/structurally-enforced primary; this by-path allowlist is the bounded residual the compiler cannot express. Registration is explicit `impl OutputProjector` source items (scanned directly, by FULL self-type path as a multiset), the owner trusted surface is a CLOSED structural inventory (exact modules + the exact impl multiset + ImplItem/TraitItem-macro + use-Sealed-alias + owner-wide TypeExpr-alias bans), and the module topology is pinned EXACTLY — construct-by-shape structural facts, not a spelling scanner.
/// hardening_rounds: 0
/// hardening_history: replaced the register_output_capability! macro-invocation inventory + the closed-leaf Item::Mod count; the impl comparison is a FULL-self-type-path MULTISET (closing the dup-last-ident gap) and the trusted-surface inventory bans ImplItem/TraitItem-macros, a use-`sealed::Sealed as` alias, and any owner-wide TypeExpr-alias (closed-allowlist completeness over the trusted owner surface). No scanner-evasion spelling hardening rounds.
/// ```
const SANCTIONED_OUTPUT_CAPS: &[&str] = &[
    "crate::host_manage::component_meta_methods::HostManageComponentMetaOutputCap",
    "crate::meta_resolve::materialize::MetaResolveFieldTypesOutputCap",
    "crate::meta_resolve::projectors::MetaResolveProjectorsOutputCap",
    "crate::resolver_core::component_meta_query_engine::MetaQueryRegistryOutputCap",
    "crate::resolver_core::component_meta_query_engine::MetaQuerySurfaceOutputCap",
    "crate::typeinfo::framework_surface::svelte_exec::TypeinfoSvelteSurfaceOutputCap",
    "crate::typeinfo::framework_surface::vue_exec::TypeinfoVueSurfaceOutputCap",
    "crate::typeinfo::raise::TypeinfoRaiseOutputCap",
];

/// The last path segment of an `impl … for <Type>` self-type, e.g.
/// `crate::meta_resolve::projectors::MetaResolveProjectorsOutputCap` ->
/// `MetaResolveProjectorsOutputCap`. Reference / group / paren self-types
/// (`&MaterializedOutputTypeExpr`, `(OutputTypeExpr)`) are unwrapped to the
/// inner path first (see the `Type::Reference`/`Group`/`Paren` arms), so a
/// carrier impl written on a reference self-type is still classified.
fn impl_self_ty_last_ident(ty: &syn::Type) -> Option<String> {
    match ty {
        syn::Type::Path(tp) => tp.path.segments.last().map(|s| s.ident.to_string()),
        // Unwrap a REFERENCE self-type to its referent so an escaping impl
        // written on `&Carrier` is still classified by carrier name. Without
        // this, `impl AsRef<TypeExpr> for &OutputTypeExpr` or
        // `impl Deref for &MaterializedOutputTypeExpr` would be SKIPPED by the
        // fence-shape inventory — a real `TypeExpr` escape (`*(&carrier)` is a
        // bare `&TypeExpr` for any holder) the privacy system does NOT catch.
        syn::Type::Reference(r) => impl_self_ty_last_ident(&r.elem),
        // Unwrap invisible grouping / explicit parentheses to the inner type
        // (`(OutputTypeExpr)`, a macro-emitted `Group`), so a parenthesised
        // carrier self-type is classified identically to the bare form.
        syn::Type::Group(g) => impl_self_ty_last_ident(&g.elem),
        syn::Type::Paren(p) => impl_self_ty_last_ident(&p.elem),
        _ => None,
    }
}

/// The FULL `::`-joined path of an `impl … for <Type>` self-type — every
/// segment ident joined with `::`, generic arguments + lifetimes STRIPPED, e.g.
/// `crate::meta_resolve::projectors::MetaResolveProjectorsOutputCap<'_, '_>` ->
/// `crate::meta_resolve::projectors::MetaResolveProjectorsOutputCap`. Reference
/// / group / paren self-types are unwrapped first (mirrors
/// [`impl_self_ty_last_ident`]). Keying the registration inventory on the FULL
/// path as a MULTISET (not the deduped last-ident) closes the duplicate-last-
/// ident gap: two impls written for `a::b::Cap` and `c::d::Cap` are now distinct
/// inventory entries (last-ident dedup would have collapsed them to one,
/// masking a second un-sanctioned same-named sink).
fn impl_self_ty_full_path(ty: &syn::Type) -> Option<String> {
    match ty {
        syn::Type::Path(tp) => {
            let joined = tp
                .path
                .segments
                .iter()
                .map(|s| s.ident.to_string())
                .collect::<Vec<_>>()
                .join("::");
            Some(joined)
        }
        syn::Type::Reference(r) => impl_self_ty_full_path(&r.elem),
        syn::Type::Group(g) => impl_self_ty_full_path(&g.elem),
        syn::Type::Paren(p) => impl_self_ty_full_path(&p.elem),
        _ => None,
    }
}

/// The EXACT set of `::`-joined module paths the owner file is allowed to
/// contain — the codex-ruled payload-vault topology: the projector seal lives
/// in `projector` (with its inert `projector::sealed` marker), the carriers +
/// their `TypeExpr` payload vault live in `carrier` (with the nested
/// `carrier::payload` vault). NOTHING ELSE. Any other inline `mod`, any
/// out-of-line `mod foo;`, or any `mod` INJECTED by an item-macro / `include!`
/// is a topology breach the old `Item::Mod`-by-kind count was blind to (a
/// `macro_rules!`-emitted `mod` or an `include!`-injected module is NOT visible
/// in the parsed AST's `Item::Mod` set).
const OWNER_MODULE_TOPOLOGY: &[&str] = &[
    "carrier",
    "carrier::payload",
    "projector",
    "projector::sealed",
];

/// One discovered owner-file module: its `::`-joined path and whether it has an
/// INLINE body (`mod m { … }`). An out-of-line `mod m;` (`has_inline_body ==
/// false`) is itself a breach — its body lives in a separate file the owner
/// scanner never parses.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct OwnerModule {
    path: String,
    has_inline_body: bool,
}

/// Recursively collect EVERY `Item::Mod` reachable in the owner file's parsed
/// AST, as `::`-joined paths. Inline modules are descended into; an out-of-line
/// `mod m;` is recorded with `has_inline_body = false` (and not descended,
/// since its body is in another file). This is the structural module-topology
/// fact the EXACT-shape guard compares against `OWNER_MODULE_TOPOLOGY`.
fn collect_owner_modules(file: &syn::File) -> Vec<OwnerModule> {
    fn walk(items: &[syn::Item], prefix: &str, out: &mut Vec<OwnerModule>) {
        for item in items {
            let syn::Item::Mod(m) = item else { continue };
            let name = m.ident.to_string();
            let path = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}::{name}")
            };
            match &m.content {
                Some((_, inner)) => {
                    out.push(OwnerModule {
                        path: path.clone(),
                        has_inline_body: true,
                    });
                    walk(inner, &path, out);
                }
                None => out.push(OwnerModule {
                    path,
                    has_inline_body: false,
                }),
            }
        }
    }
    let mut out = Vec::new();
    walk(&file.items, "", &mut out);
    out.sort();
    out
}

/// Item-position macro invocations and `include!`s the owner file is NOT
/// allowed to contain (recursively, including inside the sanctioned inline
/// modules). An item-position macro INVOCATION (`foo! { … }` / `include!(…)`)
/// can expand to a `mod` / `impl` the parsed AST walk never sees, so it must be
/// banned outright in this trusted owner surface. The SANCTIONED
/// `macro_rules! define_output_capability { … }` DEFINITION is NOT an
/// invocation (it defines a macro; `syn::ItemMacro::ident` is `Some` for a
/// `macro_rules!` definition, `None` for an invocation), so it is excluded.
///
/// ALSO recurses into `impl` and `trait` bodies and flags any `ImplItem::Macro`
/// / `TraitItem::Macro` — a macro invocation in IMPL / TRAIT position
/// (`impl X { my_macro!{} }`) can likewise expand to a hidden method / impl item
/// the AST walk never sees, so it is banned in the trusted owner surface
/// alongside the item-position invocations.
fn collect_forbidden_item_macros(file: &syn::File) -> Vec<String> {
    fn walk(items: &[syn::Item], out: &mut Vec<String>) {
        for item in items {
            match item {
                syn::Item::Macro(m) => {
                    // A `macro_rules!` DEFINITION carries an `ident`; an
                    // item-position INVOCATION (`foo!{…}`, `include!(…)`) does
                    // not. Only invocations can inject hidden items.
                    if m.ident.is_none() {
                        let name = m
                            .mac
                            .path
                            .segments
                            .last()
                            .map(|s| s.ident.to_string())
                            .unwrap_or_else(|| "<macro>".to_string());
                        out.push(name);
                    }
                }
                // An IMPL body can carry an `ImplItem::Macro` invocation that
                // expands to a hidden method / associated item.
                syn::Item::Impl(imp) => {
                    for ii in &imp.items {
                        if let syn::ImplItem::Macro(m) = ii {
                            let name = m
                                .mac
                                .path
                                .segments
                                .last()
                                .map(|s| s.ident.to_string())
                                .unwrap_or_else(|| "<impl-macro>".to_string());
                            out.push(format!("{name} (ImplItem::Macro)"));
                        }
                    }
                }
                // A TRAIT body can carry a `TraitItem::Macro` invocation.
                syn::Item::Trait(tr) => {
                    for ti in &tr.items {
                        if let syn::TraitItem::Macro(m) = ti {
                            let name = m
                                .mac
                                .path
                                .segments
                                .last()
                                .map(|s| s.ident.to_string())
                                .unwrap_or_else(|| "<trait-macro>".to_string());
                            out.push(format!("{name} (TraitItem::Macro)"));
                        }
                    }
                }
                syn::Item::Mod(syn::ItemMod {
                    content: Some((_, inner)),
                    ..
                }) => walk(inner, out),
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    walk(&file.items, &mut out);
    out
}

/// Collect any `use … sealed::Sealed as <alias>;` (or a `use …Sealed as <alias>;`
/// renaming the seal marker under a different name) ANYWHERE in the owner file.
/// The seal-impl inventory keys on the trait path's last segment being `Sealed`;
/// a `use sealed::Sealed as S; impl S for HotCap {}` would write the trait as
/// `S` and EVADE that check. The owner surface has NO legitimate need to alias
/// the private seal marker, so the alias `use` itself is banned (a tighter
/// closed inventory than alias-resolving every impl trait path).
fn collect_sealed_alias_uses(file: &syn::File) -> Vec<String> {
    fn use_tree_aliases_sealed(tree: &syn::UseTree, out: &mut Vec<String>) {
        match tree {
            syn::UseTree::Path(p) => use_tree_aliases_sealed(&p.tree, out),
            syn::UseTree::Group(g) => {
                for t in &g.items {
                    use_tree_aliases_sealed(t, out);
                }
            }
            syn::UseTree::Rename(r) => {
                // `use …::Sealed as <alias>` — the renamed source ident is
                // `Sealed`. (A `use sealed as foo` renaming the MODULE is also
                // suspicious, but the seal marker rename is the precise evasion.)
                if r.ident == "Sealed" || r.ident == "sealed" {
                    out.push(format!("use … {} as {}", r.ident, r.rename));
                }
            }
            syn::UseTree::Name(_) | syn::UseTree::Glob(_) => {}
        }
    }
    fn walk(items: &[syn::Item], out: &mut Vec<String>) {
        for item in items {
            match item {
                syn::Item::Use(u) => use_tree_aliases_sealed(&u.tree, out),
                syn::Item::Mod(syn::ItemMod {
                    content: Some((_, inner)),
                    ..
                }) => walk(inner, out),
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    walk(&file.items, &mut out);
    out
}

/// Collect any `type X = …TypeExpr…;` alias declared ANYWHERE in the owner file
/// (file scope, inside any module, OR as an `ImplItem::Type` / `TraitItem::Type`).
/// A `TypeExpr` alias in the trusted owner surface is the launder vector a
/// method-return-type recogniser misses (a fn can return `&X` instead of
/// `&TypeExpr`); the owner surface has NO legitimate `TypeExpr` alias, so the
/// alias DECLARATION is banned outright across the WHOLE owner file (broader
/// than the carrier/payload-vault-scoped ban in
/// [`carrier_uncapped_typeexpr_methods`], which only walks the vault).
fn collect_owner_typeexpr_aliases(file: &syn::File) -> Vec<String> {
    fn walk(items: &[syn::Item], out: &mut Vec<String>) {
        for item in items {
            match item {
                syn::Item::Type(t) if type_alias_aliases_type_expr(t) => {
                    out.push(t.ident.to_string());
                }
                syn::Item::Impl(imp) => {
                    for ii in &imp.items {
                        if let syn::ImplItem::Type(t) = ii {
                            if token_stream_mentions_type_expr(t.ty.to_token_stream()) {
                                out.push(format!("{} (ImplItem::Type)", t.ident));
                            }
                        }
                    }
                }
                syn::Item::Trait(tr) => {
                    for ti in &tr.items {
                        if let syn::TraitItem::Type(t) = ti {
                            if let Some((_, ty)) = &t.default {
                                if token_stream_mentions_type_expr(ty.to_token_stream()) {
                                    out.push(format!("{} (TraitItem::Type)", t.ident));
                                }
                            }
                        }
                    }
                }
                syn::Item::Mod(syn::ItemMod {
                    content: Some((_, inner)),
                    ..
                }) => walk(inner, out),
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    walk(&file.items, &mut out);
    out
}

/// The inert ATTRIBUTE allowlist for the owner surface. An attribute outside
/// this set could be an attribute PROC-MACRO that rewrites / injects items
/// (invisible to the AST walk), so an unknown attribute on any owner item is a
/// breach. These are the only attributes the trusted owner surface needs.
///
/// `cfg_attr` is INTENTIONALLY ABSENT from this broad allowlist — it is handled
/// SPECIALLY (its nested applied attribute(s) are parsed and each must itself be
/// inert), because `#[cfg_attr(unix, some_proc_macro)]` would otherwise smuggle
/// an arbitrary attribute proc-macro past the topology walk under a cfg
/// predicate. `derive` is ABSENT too: the owner file derives NOTHING (its only
/// `Clone` is a manual `impl`), so a broad `derive` allowance would needlessly
/// admit a `#[derive(SomeProcMacro)]` injection — re-add `derive` (with a nested
/// inert-trait allowlist) only if the owner file ever needs a real derive.
const ALLOWED_OWNER_ATTRS: &[&str] = &[
    "cfg", "allow", "deny", "warn", "doc", "must_use", "inline", "cold", "repr",
];

/// The INERT attributes a `#[cfg_attr(cond, <applied>…)]` is allowed to APPLY on
/// the owner surface — exactly the lint / doc / codegen-hint attributes that
/// cannot inject or rewrite items. An applied attribute outside this set (e.g.
/// `#[cfg_attr(unix, some_proc_macro)]`) is a breach: a cfg-gated attribute
/// proc-macro is invisible to the AST topology walk. NOTE: `cfg_attr` itself is
/// NOT in this set (a nested `cfg_attr` applying a `cfg_attr` is rejected —
/// the owner file never needs it).
const ALLOWED_CFG_ATTR_APPLIED: &[&str] = &[
    "allow", "deny", "warn", "doc", "must_use", "inline", "cold", "repr", "cfg",
];

/// Recursively collect any item attribute (including on items inside the
/// sanctioned inline modules, and on impl items) whose path is NOT in
/// [`ALLOWED_OWNER_ATTRS`]. A `#[doc = "…"]` rendered from a `///` comment has
/// path `doc` and is allowed. A `#[cfg_attr(cond, <applied>…)]` is parsed: the
/// `cond` predicate is skipped and each APPLIED attribute must be inert (in
/// [`ALLOWED_CFG_ATTR_APPLIED`]) — so a cfg-gated attribute proc-macro
/// (`#[cfg_attr(unix, some_proc_macro)]`) is caught.
fn collect_unknown_owner_attrs(file: &syn::File) -> Vec<String> {
    fn attr_name(a: &syn::Attribute) -> String {
        a.path()
            .segments
            .last()
            .map(|s| s.ident.to_string())
            .unwrap_or_else(|| "<attr>".to_string())
    }
    /// Last-segment ident of a nested `Meta`'s path (the applied-attribute name
    /// inside a `cfg_attr`, e.g. `allow` in `cfg_attr(not(test), allow(...))`).
    fn meta_name(m: &syn::Meta) -> String {
        m.path()
            .segments
            .last()
            .map(|s| s.ident.to_string())
            .unwrap_or_else(|| "<meta>".to_string())
    }
    /// Inspect a `cfg_attr` attribute's NESTED applied attribute(s): parse the
    /// comma-separated `Meta` list, drop the FIRST (the cfg predicate), and
    /// flag any remaining applied attribute that is NOT inert. A `cfg_attr`
    /// whose contents do not parse as a `Meta` list is itself flagged (an
    /// opaque token soup could hide an injection).
    fn check_cfg_attr(a: &syn::Attribute, out: &mut Vec<String>) {
        use syn::punctuated::Punctuated;
        let parsed = a.parse_args_with(Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated);
        let Ok(metas) = parsed else {
            out.push("cfg_attr(<unparseable>)".to_string());
            return;
        };
        // metas[0] is the cfg predicate (e.g. `not(test)`); metas[1..] are the
        // applied attributes. An empty / single-element list (no applied attr)
        // is inert (it applies nothing) but pointless; flag NOTHING for it.
        for applied in metas.iter().skip(1) {
            let name = meta_name(applied);
            if !ALLOWED_CFG_ATTR_APPLIED.contains(&name.as_str()) {
                out.push(format!("cfg_attr(…, {name})"));
            }
        }
    }
    fn check_attrs(attrs: &[syn::Attribute], out: &mut Vec<String>) {
        for a in attrs {
            let name = attr_name(a);
            if name == "cfg_attr" {
                check_cfg_attr(a, out);
                continue;
            }
            if !ALLOWED_OWNER_ATTRS.contains(&name.as_str()) {
                out.push(name);
            }
        }
    }
    fn walk(items: &[syn::Item], out: &mut Vec<String>) {
        for item in items {
            // Every item carries attrs at a known place; pull them per-kind.
            match item {
                syn::Item::Mod(m) => {
                    check_attrs(&m.attrs, out);
                    if let Some((_, inner)) = &m.content {
                        walk(inner, out);
                    }
                }
                syn::Item::Impl(imp) => {
                    check_attrs(&imp.attrs, out);
                    for ii in &imp.items {
                        match ii {
                            syn::ImplItem::Fn(f) => check_attrs(&f.attrs, out),
                            syn::ImplItem::Const(c) => check_attrs(&c.attrs, out),
                            syn::ImplItem::Type(t) => check_attrs(&t.attrs, out),
                            _ => {}
                        }
                    }
                }
                syn::Item::Struct(s) => {
                    check_attrs(&s.attrs, out);
                    for f in &s.fields {
                        check_attrs(&f.attrs, out);
                    }
                }
                syn::Item::Trait(t) => {
                    check_attrs(&t.attrs, out);
                    for ti in &t.items {
                        if let syn::TraitItem::Fn(f) = ti {
                            check_attrs(&f.attrs, out);
                        }
                    }
                }
                syn::Item::Fn(f) => check_attrs(&f.attrs, out),
                syn::Item::Use(u) => check_attrs(&u.attrs, out),
                syn::Item::Macro(m) => check_attrs(&m.attrs, out),
                syn::Item::Enum(e) => check_attrs(&e.attrs, out),
                syn::Item::Const(c) => check_attrs(&c.attrs, out),
                syn::Item::Type(t) => check_attrs(&t.attrs, out),
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    walk(&file.items, &mut out);
    out
}

/// Recursively collect the self-type last-ident of every PRODUCTION (non-test)
/// `impl OutputProjector for <Cap>` in the owner file — the EXPLICIT
/// registration impls in `mod projector`. Now that registration is explicit
/// `impl` items (not a macro), the sanctioned production sink set is exactly
/// these self-types, scanned from real source items (a hidden macro body can no
/// longer mask an extra registration). The `#[cfg(test)]`-gated
/// `impl OutputProjector for TestOutputCap` is EXCLUDED (it is test-only).
fn registered_output_projector_impls(file: &syn::File) -> Vec<String> {
    fn impl_is_test_gated(attrs: &[syn::Attribute]) -> bool {
        attrs.iter().any(|a| {
            if !a.path().is_ident("cfg") {
                return false;
            }
            matches!(&a.meta, syn::Meta::List(list)
                if cfg_is_exactly_test_or_test_support(list.tokens.clone()))
        })
    }
    fn walk(items: &[syn::Item], out: &mut Vec<String>) {
        for item in items {
            match item {
                syn::Item::Impl(imp) => {
                    let Some((_, trait_path, _)) = &imp.trait_ else {
                        continue;
                    };
                    let is_projector = trait_path
                        .segments
                        .last()
                        .map(|s| s.ident == "OutputProjector")
                        .unwrap_or(false);
                    if !is_projector {
                        continue;
                    }
                    if impl_is_test_gated(&imp.attrs) {
                        continue; // TestOutputCap registration is test-only
                    }
                    // FULL `::`-joined self-type path (multiset key) — closes the
                    // duplicate-last-ident gap.
                    if let Some(path) = impl_self_ty_full_path(&imp.self_ty) {
                        out.push(path);
                    }
                }
                syn::Item::Mod(syn::ItemMod {
                    content: Some((_, inner)),
                    ..
                }) => walk(inner, out),
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    walk(&file.items, &mut out);
    out
}

/// The result of inventorying the production (non-test) `impl sealed::Sealed`
/// impls in `mod projector`. The sealed-marker impl set must be EXACTLY the
/// same sanctioned eight as the `OutputProjector` impl set (a sealed impl for a
/// non-sanctioned type, or a blanket `impl<T> sealed::Sealed for T`, would let
/// that type satisfy the `OutputProjector: sealed::Sealed` supertrait bound and
/// become a capability).
#[derive(Debug, Default, PartialEq, Eq)]
struct SealedImplInventory {
    /// The concrete self-type last-idents of every non-test `impl sealed::Sealed
    /// for <Cap>`.
    concrete_self_types: Vec<String>,
    /// Messages for any blanket / generic `impl<…> sealed::Sealed for <T>` whose
    /// self-type is one of the impl's own generic type params (a blanket impl
    /// seals an OPEN set of types — the exact residual [P2-1]'s private
    /// `mod sealed` does NOT itself prevent inside `mod projector`).
    blanket_violations: Vec<String>,
}

/// Recursively inventory every PRODUCTION (non-test) `impl sealed::Sealed for
/// <T>` in the owner file (the explicit seal impls in `mod projector`). With
/// [P2-1]'s private `mod sealed`, a `sealed::Sealed` impl can ONLY be written
/// inside `mod projector`; the residual this catches is (a) a seal impl for a
/// type OUTSIDE the sanctioned eight, and (b) a blanket `impl<T>
/// sealed::Sealed for T {}` that seals an open set of types. The
/// `#[cfg(test)]`-gated `impl sealed::Sealed for TestOutputCap` is EXCLUDED.
fn registered_sealed_impls(file: &syn::File) -> SealedImplInventory {
    fn impl_is_test_gated(attrs: &[syn::Attribute]) -> bool {
        attrs.iter().any(|a| {
            if !a.path().is_ident("cfg") {
                return false;
            }
            matches!(&a.meta, syn::Meta::List(list)
                if cfg_is_exactly_test_or_test_support(list.tokens.clone()))
        })
    }
    /// Is the trait path's LAST segment `Sealed` AND its penultimate segment
    /// `sealed` (i.e. `…sealed::Sealed`)? Keying on the two-segment tail avoids
    /// matching an unrelated `Sealed` trait from elsewhere.
    fn trait_is_sealed_marker(path: &syn::Path) -> bool {
        let n = path.segments.len();
        if n == 0 {
            return false;
        }
        let last_is_sealed = path.segments[n - 1].ident == "Sealed";
        // Accept both `sealed::Sealed` (n>=2) and a bare `Sealed` (n==1) so a
        // future `use sealed::Sealed as Sealed;`-aliased impl is still caught by
        // last-segment; require the penultimate to be `sealed` when present.
        let penultimate_ok = n < 2 || path.segments[n - 2].ident == "sealed";
        last_is_sealed && penultimate_ok
    }
    fn walk(items: &[syn::Item], inv: &mut SealedImplInventory) {
        for item in items {
            match item {
                syn::Item::Impl(imp) => {
                    let Some((_, trait_path, _)) = &imp.trait_ else {
                        continue;
                    };
                    if !trait_is_sealed_marker(trait_path) {
                        continue;
                    }
                    if impl_is_test_gated(&imp.attrs) {
                        continue; // TestOutputCap seal is test-only
                    }
                    // Collect the impl's own generic TYPE-param idents (lifetimes
                    // don't make a blanket impl; a TYPE param used AS the
                    // self-type does).
                    let type_params: Vec<String> = imp
                        .generics
                        .params
                        .iter()
                        .filter_map(|p| match p {
                            syn::GenericParam::Type(tp) => Some(tp.ident.to_string()),
                            _ => None,
                        })
                        .collect();
                    let self_name = impl_self_ty_last_ident(&imp.self_ty)
                        .unwrap_or_else(|| "<impl>".to_string());
                    // A blanket impl: the self-type IS one of the impl's generic
                    // type params (`impl<T> sealed::Sealed for T`).
                    if type_params.contains(&self_name) {
                        inv.blanket_violations.push(format!(
                            "blanket/generic `impl<{}> sealed::Sealed for {self_name}` — a generic \
                             seal impl seals an OPEN set of types, letting any of them satisfy the \
                             `OutputProjector: sealed::Sealed` supertrait bound. Seal EXACTLY the \
                             eight sanctioned per-leaf capability types, never a type parameter",
                            type_params.join(", ")
                        ));
                    } else {
                        // FULL `::`-joined self-type path (multiset key) — closes
                        // the duplicate-last-ident gap.
                        let full = impl_self_ty_full_path(&imp.self_ty)
                            .unwrap_or_else(|| self_name.clone());
                        inv.concrete_self_types.push(full);
                    }
                }
                syn::Item::Mod(syn::ItemMod {
                    content: Some((_, inner)),
                    ..
                }) => walk(inner, inv),
                _ => {}
            }
        }
    }
    let mut inv = SealedImplInventory::default();
    walk(&file.items, &mut inv);
    inv
}

/// The structs in the carrier/payload vault whose fields must ALL be private —
/// the inner-`TypeExpr` carriers + the payload newtype. EVERY field of each
/// must be `Visibility::Inherited`, REGARDLESS of the spelled field type: this
/// catches both a widened `pub`/`pub(crate)` payload field AND the
/// `type Inner = TypeExpr; struct OutputPayload(pub Inner)` alias launder (the
/// vis is read structurally, never keyed on the spelled type name).
const VAULT_PRIVATE_FIELD_STRUCTS: &[&str] = &[
    "OutputPayload",
    "OutputTypeExpr",
    "MaterializedOutputTypeExpr",
];

/// Recursively find any field of a [`VAULT_PRIVATE_FIELD_STRUCTS`] struct whose
/// visibility is NOT private (`Visibility::Inherited`). A non-private field
/// would let a holder read the inner `TypeExpr` (or the payload that wraps it)
/// capability-free, regardless of how the field type is spelled.
fn vault_nonprivate_fields(file: &syn::File) -> Vec<String> {
    fn walk(items: &[syn::Item], out: &mut Vec<String>) {
        for item in items {
            match item {
                syn::Item::Struct(s) => {
                    let name = s.ident.to_string();
                    if !VAULT_PRIVATE_FIELD_STRUCTS.contains(&name.as_str()) {
                        continue;
                    }
                    for (idx, field) in s.fields.iter().enumerate() {
                        if !matches!(field.vis, syn::Visibility::Inherited) {
                            let fname = field
                                .ident
                                .as_ref()
                                .map(|i| i.to_string())
                                .unwrap_or_else(|| format!("<tuple.{idx}>"));
                            out.push(format!(
                                "`{name}` has a non-private field `{fname}` — every field of the \
                                 carrier/payload vault MUST be private (capability-gated unwrap \
                                 only); a non-private field reads the inner `TypeExpr` (or its \
                                 wrapping payload) capability-free, regardless of the spelled type"
                            ));
                        }
                    }
                }
                syn::Item::Mod(syn::ItemMod {
                    content: Some((_, inner)),
                    ..
                }) => walk(inner, out),
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    walk(&file.items, &mut out);
    out
}

/// Does a fn signature carry the sanctioned CAPABILITY gate — a generic type
/// parameter bounded by `OutputProjector` (the `<P: OutputProjector + ?Sized>`
/// shape every gated accessor uses)? This is the structural proof the read is
/// unwrap-local; a `fn leak(&self) -> &TypeExpr` with no such bound is NOT
/// capability-gated and is a laundering escape regardless of its name.
fn sig_has_output_projector_capability(sig: &syn::Signature) -> bool {
    sig.generics.params.iter().any(|p| {
        let syn::GenericParam::Type(tp) = p else {
            return false;
        };
        tp.bounds.iter().any(|b| {
            let syn::TypeParamBound::Trait(tb) = b else {
                return false;
            };
            tb.path
                .segments
                .last()
                .map(|s| s.ident == "OutputProjector")
                .unwrap_or(false)
        })
    })
}

/// Does a fn's attribute set make it EXACTLY a production-unreachable test
/// accessor (`#[cfg(test)]` / `#[cfg(any(test, feature = "test-support"))]`)?
/// Reuses the shared EXACT cfg recogniser.
fn fn_is_exactly_test_gated(attrs: &[syn::Attribute]) -> bool {
    let cfgs: Vec<&syn::Attribute> = attrs.iter().filter(|a| a.path().is_ident("cfg")).collect();
    if cfgs.len() != 1 {
        return false;
    }
    let syn::Meta::List(list) = &cfgs[0].meta else {
        return false;
    };
    cfg_is_exactly_test_or_test_support(list.tokens.clone())
}

/// Does a token stream mention the `TypeExpr` ident ANYWHERE — recursively
/// descending every nested `Group` (`&TypeExpr`, `Option<TypeExpr>`,
/// `Vec<Box<TypeExpr>>`, …)? The shared TypeExpr-mention recogniser the
/// return-type and type-alias checks both key on.
fn token_stream_mentions_type_expr(tokens: proc_macro2::TokenStream) -> bool {
    tokens.into_iter().any(|tt| match tt {
        proc_macro2::TokenTree::Ident(id) => id == "TypeExpr",
        proc_macro2::TokenTree::Group(g) => token_stream_mentions_type_expr(g.stream()),
        _ => false,
    })
}

/// Does a return type mention `TypeExpr` (as `TypeExpr`, `&TypeExpr`,
/// `Option<TypeExpr>`, …)? Checked on the return type's token stream so any
/// `TypeExpr`-bearing return is caught.
fn return_type_mentions_type_expr(output: &syn::ReturnType) -> bool {
    let syn::ReturnType::Type(_, ty) = output else {
        return false;
    };
    token_stream_mentions_type_expr(ty.to_token_stream())
}

/// Does a `type X = …;` alias's RIGHT-HAND-SIDE mention `TypeExpr` (directly or
/// nested, e.g. `type Inner = TypeExpr` / `type Inner = Option<TypeExpr>`)? A
/// vault-scoped alias to `TypeExpr` is the launder vector the inherent-method
/// allowlist alone misses: a method `fn alias_leak(&self) -> &Inner` returns
/// `&Inner`, NOT `&TypeExpr`, so the return-type recogniser does not fire — the
/// inner `TypeExpr` escapes capability-free. The vault has NO legitimate use
/// for a `TypeExpr` alias, so the alias DECLARATION is itself banned (a tighter
/// closed inventory than alias-resolving every method return). Banning the
/// ROOT `type X = …TypeExpr…` also breaks any transitive alias chain at its
/// source: a `type B = A` chain cannot bear `TypeExpr` without some root
/// `type A = …TypeExpr…` the ban catches.
fn type_alias_aliases_type_expr(item_type: &syn::ItemType) -> bool {
    token_stream_mentions_type_expr(item_type.ty.to_token_stream())
}

/// CLOSED ITEM/SIGNATURE ALLOWLIST over the carrier + payload vault modules:
/// every production `fn` (inherent OR in any impl) whose RETURN type mentions
/// `TypeExpr` / `&TypeExpr` MUST be either (a) capability-gated (a
/// `P: OutputProjector` generic bound — the sanctioned `into_type_expr` /
/// `type_expr` accessor shape), or (b) EXACTLY test-gated (the `*_for_test`
/// accessor under `#[cfg(test)]` / `#[cfg(any(test, feature = "test-support"))]`).
/// Any other `fn` returning `TypeExpr` — `fn raw/leak/payload(&self) ->
/// &TypeExpr` with no cap param, regardless of name — is a laundering escape.
/// This is the closed allowlist that replaces the old finite name-blacklist
/// (`into_inner` / `as_type_expr` / `as_inner`), which an unlisted name could
/// evade. It walks only the carrier/payload modules (the trusted vault); the
/// topology guard guarantees no OTHER module exists in the owner file.
///
/// ALSO bans any `type X = …TypeExpr…` alias DECLARED inside the vault: a
/// `type Inner = TypeExpr; fn alias_leak(&self) -> &Inner` returns `&Inner`
/// (not `&TypeExpr`), so the return-type recogniser alone would miss it — the
/// alias is the launder vector. The vault has no legitimate `TypeExpr` alias,
/// so the alias is banned at its declaration (a tighter closed inventory),
/// mirroring the field-privacy guard's structural alias defense. This brings
/// the inherent-method guard to the same robustness — the two are no longer
/// asymmetric on the alias trick.
fn carrier_uncapped_typeexpr_methods(file: &syn::File) -> Vec<String> {
    // Walk into `carrier` and its nested `payload` module; inspect every fn in
    // every impl block (and any free fn) for a `TypeExpr`-bearing return.
    fn inspect_fn(
        owner_label: &str,
        ident: &syn::Ident,
        sig: &syn::Signature,
        attrs: &[syn::Attribute],
        out: &mut Vec<String>,
    ) {
        if !return_type_mentions_type_expr(&sig.output) {
            return;
        }
        let capability_gated = sig_has_output_projector_capability(sig);
        let test_gated = fn_is_exactly_test_gated(attrs);
        if !capability_gated && !test_gated {
            out.push(format!(
                "`{owner_label}::{ident}` returns a `TypeExpr` / `&TypeExpr` but is NEITHER \
                 capability-gated (a `P: OutputProjector` bound) NOR exactly test-gated — it is an \
                 un-gated inner-`TypeExpr` accessor (the only sanctioned readers are the \
                 capability-gated `into_type_expr` / `type_expr`, plus the test-only `*_for_test`)"
            ));
        }
    }
    fn walk(items: &[syn::Item], in_vault: bool, out: &mut Vec<String>) {
        for item in items {
            match item {
                syn::Item::Mod(m) => {
                    let name = m.ident.to_string();
                    let entering_vault = in_vault || name == "carrier" || name == "payload";
                    if let Some((_, inner)) = &m.content {
                        walk(inner, entering_vault, out);
                    }
                }
                syn::Item::Impl(imp) if in_vault => {
                    let owner = impl_self_ty_last_ident(&imp.self_ty)
                        .unwrap_or_else(|| "<impl>".to_string());
                    for ii in &imp.items {
                        if let syn::ImplItem::Fn(f) = ii {
                            inspect_fn(&owner, &f.sig.ident, &f.sig, &f.attrs, out);
                        }
                    }
                }
                syn::Item::Fn(f) if in_vault => {
                    inspect_fn("<free>", &f.sig.ident, &f.sig, &f.attrs, out);
                }
                // A `type X = …TypeExpr…` alias declared inside the vault is the
                // alias-launder vector: it lets a method return `&X` instead of
                // `&TypeExpr`, evading the return-type recogniser. Ban it at the
                // declaration (the vault has no legitimate `TypeExpr` alias).
                syn::Item::Type(t) if in_vault => {
                    if type_alias_aliases_type_expr(t) {
                        out.push(format!(
                            "`type {} = …` aliases `TypeExpr` inside the carrier/payload vault — a \
                             `TypeExpr` alias in the vault is a laundering vector (a method can \
                             return the alias instead of `TypeExpr` to evade the return-type \
                             allowlist); the vault has NO legitimate `TypeExpr` alias, so it is \
                             banned at the declaration",
                            t.ident
                        ));
                    }
                }
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    walk(&file.items, false, &mut out);
    out
}

#[test]
fn output_projector_owner_registration_inventory() {
    let src = read_rel(OWNER_REL);
    let file = syn::parse_file(&src).expect("parse output_materialization.rs");

    // (1) Owner registration inventory: the PRODUCTION output-sink set is
    // EXACTLY the eight sanctioned `impl OutputProjector for <Cap>` FULL
    // self-type paths, compared as a MULTISET (sorted, NOT deduped) — a
    // duplicate registration (the same path twice, or a second sink whose LAST
    // ident collides with a sanctioned one) is a length/element mismatch that
    // FIRES. Registration is EXPLICIT `impl` pairs in `mod projector` (NOT a
    // macro), so we scan the real source impls by full self-type path — a hidden
    // macro body can no longer mask an extra registration, and the dup-last-
    // ident gap is closed. The `#[cfg(test)]` `TestOutputCap` impl is excluded
    // by the test-gate check.
    let mut observed = registered_output_projector_impls(&file);
    observed.sort();
    let mut expected: Vec<String> = SANCTIONED_OUTPUT_CAPS
        .iter()
        .map(|s| s.to_string())
        .collect();
    expected.sort();
    assert_eq!(
        observed, expected,
        "the PRODUCTION `impl OutputProjector for <Cap>` set must be EXACTLY the eight sanctioned \
         output-sink capability FULL self-type paths (multiset); a new explicit registration impl \
         for any other type — OR a duplicate / last-ident-colliding path — widens the laundering \
         surface (route new sinks through an EXISTING capability, or add the capability + its impl \
         pair + this row deliberately)"
    );

    // (1b) [P2.4] SEALED-MARKER inventory: the PRODUCTION `impl sealed::Sealed
    // for <Cap>` set must ALSO be EXACTLY the same sanctioned eight, AND there
    // must be NO blanket/generic `impl<T> sealed::Sealed for T`. The
    // `OutputProjector` inventory above pins the trait impls; this pins the
    // SUPERTRAIT-seal impls, so an extra `impl sealed::Sealed for OtherType`
    // (which alone makes `OtherType` eligible for an `OutputProjector` impl) or
    // a blanket seal (which seals an open set) is caught. With [P2-1]'s private
    // `mod sealed`, such an impl can only be written inside `mod projector`,
    // which this walk covers.
    let sealed_inv = registered_sealed_impls(&file);
    assert!(
        sealed_inv.blanket_violations.is_empty(),
        "blanket/generic `impl sealed::Sealed` violation(s) — a generic seal impl seals an OPEN \
         set of types into the `OutputProjector` supertrait bound:\n{}",
        sealed_inv.blanket_violations.join("\n")
    );
    let mut observed_sealed = sealed_inv.concrete_self_types;
    observed_sealed.sort();
    assert_eq!(
        observed_sealed, expected,
        "the PRODUCTION `impl sealed::Sealed for <Cap>` set must be EXACTLY the eight sanctioned \
         per-leaf output-sink capabilities — the SAME set as the `impl OutputProjector` impls. An \
         `impl sealed::Sealed` for any other concrete type makes that type eligible to satisfy the \
         `OutputProjector: sealed::Sealed` supertrait bound (a laundering widening); seal EXACTLY \
         the sanctioned eight"
    );

    // (2) EXACT MODULE TOPOLOGY: the owner file's module shape is EXACTLY the
    // codex-ruled payload-vault topology — inline `mod projector`, inline
    // `mod carrier`, the nested inline vaults `carrier::payload` /
    // `projector::sealed`, and NOTHING ELSE. The old `Item::Mod`-by-kind count
    // was blind to macro/include INJECTION: a `macro_rules!`-emitted `mod` or an
    // `include!`-injected module never appears as an `Item::Mod` in the parsed
    // AST. So this guard pins the EXACT inline-module set AND bans item-position
    // macro invocations / `include!` / unknown attribute macros (any of which
    // could inject a hidden `mod` / `impl` reaching the private
    // `projector::sealed` marker or the carrier/payload vault).
    let modules = collect_owner_modules(&file);
    let mut topology_violations: Vec<String> = Vec::new();
    // (2a) Every discovered module must be an ALLOWED path AND inline.
    for m in &modules {
        if !OWNER_MODULE_TOPOLOGY.contains(&m.path.as_str()) {
            topology_violations.push(format!(
                "unexpected module `mod {}` — the owner file's module topology is EXACTLY \
                 {{projector, projector::sealed, carrier, carrier::payload}}; any other module is \
                 an owner-descendant scope that could reach the private `projector::sealed` marker \
                 or the carrier/payload vault",
                m.path
            ));
        } else if !m.has_inline_body {
            topology_violations.push(format!(
                "module `mod {};` is out-of-line — its body lives in a separate file the owner \
                 scanner never parses; the vault topology MUST be inline so its exact shape is \
                 reviewable here",
                m.path
            ));
        }
    }
    // (2b) Every ALLOWED module must EXIST (anti-vacuity: a removed vault module
    // means the seal / vault is gone, not a pass).
    let observed_paths: std::collections::BTreeSet<&str> =
        modules.iter().map(|m| m.path.as_str()).collect();
    for expected_mod in OWNER_MODULE_TOPOLOGY {
        if !observed_paths.contains(expected_mod) {
            topology_violations.push(format!(
                "anti-vacuity: the required vault module `mod {expected_mod}` was NOT found in the \
                 owner file's inline module topology — the projector seal / carrier payload vault \
                 is missing or moved"
            ));
        }
    }
    // (2c) No item-position macro INVOCATION / `include!` (the sanctioned
    // `macro_rules! define_output_capability` DEFINITION is allowed).
    let forbidden_macros = collect_forbidden_item_macros(&file);
    for name in &forbidden_macros {
        topology_violations.push(format!(
            "item-position macro invocation `{name}!` in the owner file — an item macro / \
             `include!` can expand to a hidden `mod` / `impl` the AST topology walk never sees, so \
             it is BANNED in this trusted owner surface (the only sanctioned macro item is the \
             `macro_rules! define_output_capability` DEFINITION)"
        ));
    }
    // (2d) No unknown attribute (an attribute proc-macro could inject items).
    let unknown_attrs = collect_unknown_owner_attrs(&file);
    for name in &unknown_attrs {
        topology_violations.push(format!(
            "unknown attribute `#[{name}]` on an owner item — an attribute proc-macro could rewrite \
             or inject items invisibly to the topology walk; only inert attributes \
             ({}) are allowed in this trusted owner surface",
            ALLOWED_OWNER_ATTRS.join(", ")
        ));
    }
    // (2e) No `ImplItem::Macro` / `TraitItem::Macro` invocation (covered by the
    // extended `collect_forbidden_item_macros`, which now recurses impl/trait
    // bodies — a macro in impl/trait position can expand to a hidden method /
    // associated item). The (2c) loop already drained `forbidden_macros`, which
    // includes the impl/trait-position invocations.
    //
    // (2f) No `use …sealed::Sealed as <alias>;` rename of the seal marker — it
    // would let `impl <alias> for HotCap {}` evade the last-segment-`Sealed`
    // seal-impl inventory.
    for alias in collect_sealed_alias_uses(&file) {
        topology_violations.push(format!(
            "`{alias}` in the owner file — aliasing the private `sealed::Sealed` seal marker lets \
             an `impl <alias> for HotCap {{}}` evade the last-segment-`Sealed` seal-impl inventory; \
             the owner surface has NO legitimate need to alias the seal marker, so the alias `use` \
             is banned"
        ));
    }
    // (2g) No `type X = …TypeExpr…;` alias ANYWHERE in the owner file — a
    // `TypeExpr` alias is the launder vector a method-return-type recogniser
    // misses (a fn returns `&X`, not `&TypeExpr`). The owner surface has NO
    // legitimate `TypeExpr` alias, so it is banned across the WHOLE file (the
    // vault-scoped ban in `carrier_uncapped_typeexpr_methods` only walks the
    // carrier/payload modules; this closes the rest of the owner surface).
    for alias in collect_owner_typeexpr_aliases(&file) {
        topology_violations.push(format!(
            "`type {alias}` mentions `TypeExpr` in the owner file — a `TypeExpr` alias in the \
             trusted owner surface is a laundering vector (a fn can return the alias instead of \
             `TypeExpr` to evade the return-type allowlist); the owner surface has NO legitimate \
             `TypeExpr` alias, so it is banned at the declaration"
        ));
    }
    assert!(
        topology_violations.is_empty(),
        "OWNER MODULE-TOPOLOGY / CLOSED-INVENTORY violation(s) — the owner file's exact vault shape \
         or trusted-surface inventory is breached (a topology change, a macro/include injection in \
         item/impl/trait position, a `sealed::Sealed` alias, or a `TypeExpr` alias — the \
         trusted-surface residual the compiler cannot express):\n{}",
        topology_violations.join("\n")
    );
}

#[test]
fn output_projector_owner_registration_inventory_self_test_discriminates() {
    // Registration-inventory detector: the set of explicit
    // `impl OutputProjector for <Cap>` self-types must be collected exactly.
    fn impls(src: &str) -> Vec<String> {
        let file = syn::parse_file(src).expect("parse synthetic");
        let mut c = registered_output_projector_impls(&file);
        c.sort();
        c
    }
    // RED: an EXTRA explicit registration impl (a ninth, non-sanctioned sink) is
    // observed — the real guard's `assert_eq!` against the sanctioned eight FAILS.
    let extra = r#"
        mod projector {
            impl sealed::Sealed for crate::x::SanctionedCap<'_, '_> {}
            impl OutputProjector for crate::x::SanctionedCap<'_, '_> {
                fn dispatch(&self) -> &ProjectSemanticDispatch<'_> { self.dispatch_for_projector() }
            }
            impl sealed::Sealed for crate::x::SneakyNewSink<'_, '_> {}
            impl OutputProjector for crate::x::SneakyNewSink<'_, '_> {
                fn dispatch(&self) -> &ProjectSemanticDispatch<'_> { self.dispatch_for_projector() }
            }
        }
    "#;
    assert_eq!(
        impls(extra),
        vec![
            "crate::x::SanctionedCap".to_string(),
            "crate::x::SneakyNewSink".to_string()
        ],
        "self-test: an extra explicit `impl OutputProjector for <Cap>` MUST be observed by FULL \
         self-type path (it widens the sanctioned sink set the inventory pins)"
    );
    // RED (dup-last-ident): two impls whose self-types share a LAST ident but
    // differ in FULL path are TWO distinct multiset entries — the prior
    // last-ident `dedup()` collapsed them to one, masking the second sink. The
    // full-path collector keeps both.
    let dup_last_ident = r#"
        mod projector {
            impl OutputProjector for crate::a::Cap<'_, '_> {
                fn dispatch(&self) -> &ProjectSemanticDispatch<'_> { self.dispatch_for_projector() }
            }
            impl OutputProjector for crate::b::Cap<'_, '_> {
                fn dispatch(&self) -> &ProjectSemanticDispatch<'_> { self.dispatch_for_projector() }
            }
        }
    "#;
    assert_eq!(
        impls(dup_last_ident),
        vec!["crate::a::Cap".to_string(), "crate::b::Cap".to_string()],
        "self-test: two `impl OutputProjector` with the SAME last ident (`Cap`) but DIFFERENT full \
         paths MUST be observed as TWO entries (the dup-last-ident gap the old last-ident dedup \
         masked)"
    );
    // PASS: a `#[cfg(test)]` `impl OutputProjector for TestOutputCap` is
    // test-gated, so it is correctly EXCLUDED from the production set.
    let test_impl = r#"
        mod projector {
            #[cfg(test)]
            impl OutputProjector for TestOutputCap<'_, '_> {
                fn dispatch(&self) -> &ProjectSemanticDispatch<'_> { self.dispatch }
            }
        }
    "#;
    assert!(
        impls(test_impl).is_empty(),
        "self-test: a `#[cfg(test)] impl OutputProjector for TestOutputCap` is test-gated and MUST \
         NOT enter the production sink set"
    );

    // [P2.4] SEALED-MARKER inventory detector: the set of `impl sealed::Sealed
    // for <Cap>` concrete self-types must be collected, and a blanket/generic
    // seal impl must be flagged separately.
    fn sealed(src: &str) -> SealedImplInventory {
        let file = syn::parse_file(src).expect("parse synthetic");
        let mut inv = registered_sealed_impls(&file);
        inv.concrete_self_types.sort();
        inv
    }
    // RED [P2.4]: a blanket `impl<T> sealed::Sealed for T {}` is flagged as a
    // blanket violation (the residual private `mod sealed` does NOT itself stop
    // INSIDE `mod projector`).
    let blanket = r#"
        mod projector {
            mod sealed { pub trait Sealed {} }
            impl<T> sealed::Sealed for T {}
        }
    "#;
    assert!(
        sealed(blanket)
            .blanket_violations
            .iter()
            .any(|m| m.contains("blanket/generic") && m.contains("for T")),
        "self-test: a blanket `impl<T> sealed::Sealed for T {{}}` MUST fire the blanket-impl check; \
         got {:?}",
        sealed(blanket)
    );
    // RED [P2.4]: an EXTRA concrete `impl sealed::Sealed for OtherType` (a ninth
    // sealed type) is observed — the real guard's `assert_eq!` against the
    // sanctioned eight FAILS.
    let extra_sealed = r#"
        mod projector {
            mod sealed { pub trait Sealed {} }
            impl sealed::Sealed for crate::x::SanctionedCap<'_, '_> {}
            impl sealed::Sealed for crate::x::SneakyNewSink<'_, '_> {}
        }
    "#;
    assert_eq!(
        sealed(extra_sealed).concrete_self_types,
        vec![
            "crate::x::SanctionedCap".to_string(),
            "crate::x::SneakyNewSink".to_string()
        ],
        "self-test: an extra concrete `impl sealed::Sealed for <Cap>` MUST be observed by FULL \
         self-type path (it widens the sealed set the inventory pins)"
    );
    // PASS [P2.4]: a `#[cfg(test)] impl sealed::Sealed for TestOutputCap` is
    // test-gated and EXCLUDED from the production sealed set.
    let test_sealed = r#"
        mod projector {
            mod sealed { pub trait Sealed {} }
            #[cfg(test)]
            impl sealed::Sealed for TestOutputCap<'_, '_> {}
        }
    "#;
    assert!(
        sealed(test_sealed).concrete_self_types.is_empty()
            && sealed(test_sealed).blanket_violations.is_empty(),
        "self-test: a `#[cfg(test)] impl sealed::Sealed for TestOutputCap` is test-gated and MUST \
         NOT enter the production sealed set; got {:?}",
        sealed(test_sealed)
    );
    // PASS [P2.4]: a NON-blanket generic seal impl with a concrete self-type
    // (`impl<'a> sealed::Sealed for Cap<'a>`) is NOT a blanket violation — the
    // self-type is concrete, only the lifetime is generic.
    let lifetime_generic = r#"
        mod projector {
            mod sealed { pub trait Sealed {} }
            impl sealed::Sealed for crate::x::Cap<'_, '_> {}
        }
    "#;
    assert!(
        sealed(lifetime_generic).blanket_violations.is_empty()
            && sealed(lifetime_generic).concrete_self_types == vec!["crate::x::Cap".to_string()],
        "self-test: a lifetime-generic seal impl with a CONCRETE self-type MUST NOT be a blanket \
         violation; got {:?}",
        sealed(lifetime_generic)
    );

    // Module-topology detector: the EXACT inline-module set must be enforced,
    // and macro/include injection + out-of-line modules must fire.
    fn topo_violations(src: &str) -> Vec<String> {
        let file = syn::parse_file(src).expect("parse synthetic");
        let mut v = Vec::new();
        for m in collect_owner_modules(&file) {
            if !OWNER_MODULE_TOPOLOGY.contains(&m.path.as_str()) {
                v.push(format!("unexpected mod {}", m.path));
            } else if !m.has_inline_body {
                v.push(format!("out-of-line mod {}", m.path));
            }
        }
        for name in collect_forbidden_item_macros(&file) {
            v.push(format!("forbidden item macro {name}!"));
        }
        for name in collect_unknown_owner_attrs(&file) {
            v.push(format!("unknown attr #[{name}]"));
        }
        for alias in collect_sealed_alias_uses(&file) {
            v.push(format!("sealed alias {alias}"));
        }
        for alias in collect_owner_typeexpr_aliases(&file) {
            v.push(format!("typeexpr alias {alias}"));
        }
        v
    }

    // RED: an EXTRA top-level `mod shadow { … }` (the owner-descendant laundering
    // vector) fires.
    let shadow = r#"
        mod projector { mod sealed { pub trait Sealed {} } }
        mod carrier { mod payload {} }
        mod shadow {
            impl super::projector::sealed::Sealed for super::carrier::OutputTypeExpr {}
        }
    "#;
    assert!(
        topo_violations(shadow).iter().any(|m| m.contains("shadow")),
        "self-test: a top-level `mod shadow {{ … }}` MUST fire the topology check; got {:?}",
        topo_violations(shadow)
    );
    // RED: a `macro_rules!`-emitted module (item-position macro INVOCATION) is
    // BANNED — the exact macro-injection gap the old `Item::Mod` count missed.
    let macro_injected = r#"
        mod projector { mod sealed { pub trait Sealed {} } }
        mod carrier { mod payload {} }
        emit_a_module!{ mod sneaky { impl super::projector::sealed::Sealed for X {} } }
    "#;
    assert!(
        topo_violations(macro_injected)
            .iter()
            .any(|m| m.contains("emit_a_module")),
        "self-test: an item-position macro invocation (which can inject a hidden `mod`) MUST fire \
         the topology check; got {:?}",
        topo_violations(macro_injected)
    );
    // RED: an `include!` item macro is BANNED.
    let include_injected = r#"
        mod projector { mod sealed { pub trait Sealed {} } }
        mod carrier { mod payload {} }
        include!("sneaky_impls.rs");
    "#;
    assert!(
        topo_violations(include_injected)
            .iter()
            .any(|m| m.contains("include")),
        "self-test: an `include!` item macro MUST fire the topology check; got {:?}",
        topo_violations(include_injected)
    );
    // RED: an out-of-line `mod carrier;` (body in a separate file) fires.
    let out_of_line = r#"
        mod projector { mod sealed { pub trait Sealed {} } }
        mod carrier;
    "#;
    assert!(
        topo_violations(out_of_line)
            .iter()
            .any(|m| m.contains("out-of-line mod carrier")),
        "self-test: an out-of-line `mod carrier;` MUST fire the topology check; got {:?}",
        topo_violations(out_of_line)
    );
    // RED: an unknown attribute (a possible attribute proc-macro) on an owner
    // item fires.
    let unknown_attr = r#"
        mod projector { mod sealed { pub trait Sealed {} } }
        mod carrier { mod payload {} }
        #[sneaky_proc_macro]
        struct Decoy;
    "#;
    assert!(
        topo_violations(unknown_attr)
            .iter()
            .any(|m| m.contains("sneaky_proc_macro")),
        "self-test: an unknown attribute (possible proc-macro) MUST fire the topology check; got \
         {:?}",
        topo_violations(unknown_attr)
    );
    // RED [P2.3]: a `#[cfg_attr(unix, some_unknown_macro)]` smuggling an
    // attribute proc-macro under a cfg predicate fires — the nested-content hole
    // the old broad `cfg_attr` allowance was blind to (it admitted any
    // `cfg_attr` without inspecting the applied attribute).
    let cfg_attr_proc_macro = r#"
        mod projector { mod sealed { pub trait Sealed {} } }
        mod carrier { mod payload {} }
        #[cfg_attr(unix, some_unknown_macro)]
        struct Decoy;
    "#;
    assert!(
        topo_violations(cfg_attr_proc_macro)
            .iter()
            .any(|m| m.contains("cfg_attr") && m.contains("some_unknown_macro")),
        "self-test: a `#[cfg_attr(unix, some_unknown_macro)]` (cfg-gated attribute proc-macro) MUST \
         fire — the nested applied attribute is not inert; got {:?}",
        topo_violations(cfg_attr_proc_macro)
    );
    // RED [P2.3]: a `cfg_attr` applying a `derive` of an arbitrary (possibly
    // proc-macro) trait fires (`derive` is NOT on the inert applied allowlist).
    let cfg_attr_derive = r#"
        mod projector { mod sealed { pub trait Sealed {} } }
        mod carrier { mod payload {} }
        #[cfg_attr(feature = "x", derive(SomeProcMacro))]
        struct Decoy;
    "#;
    assert!(
        topo_violations(cfg_attr_derive)
            .iter()
            .any(|m| m.contains("cfg_attr") && m.contains("derive")),
        "self-test: a `#[cfg_attr(…, derive(SomeProcMacro))]` MUST fire — `derive` is not an inert \
         applied attribute on the owner surface; got {:?}",
        topo_violations(cfg_attr_derive)
    );
    // RED [P2.3]: a bare `#[derive(SomeProcMacro)]` (derive removed from the
    // broad allowlist — the owner file derives nothing) fires.
    let bare_derive = r#"
        mod projector { mod sealed { pub trait Sealed {} } }
        mod carrier { mod payload {} }
        #[derive(SomeProcMacro)]
        struct Decoy;
    "#;
    assert!(
        topo_violations(bare_derive)
            .iter()
            .any(|m| m.contains("derive")),
        "self-test: a bare `#[derive(SomeProcMacro)]` MUST fire — `derive` is no longer broadly \
         allowed on the owner surface (it derives nothing); got {:?}",
        topo_violations(bare_derive)
    );
    // PASS [P2.3]: the LEGITIMATE owner `#[cfg_attr(not(test), allow(dead_code))]`
    // (the real shape on the `node_id()` accessor) does NOT fire — its applied
    // attribute `allow` is inert.
    let cfg_attr_inert = r#"
        mod projector { mod sealed { pub trait Sealed {} } }
        mod carrier {
            mod payload {}
            impl MaterializedOutputTypeExpr {
                #[cfg_attr(not(test), allow(dead_code))]
                pub(crate) fn node_id(&self) -> Option<SemanticNodeId> { self.node_id }
            }
        }
    "#;
    assert!(
        topo_violations(cfg_attr_inert).is_empty(),
        "self-test: the legitimate `#[cfg_attr(not(test), allow(dead_code))]` (inert applied \
         `allow`) MUST pass; got {:?}",
        topo_violations(cfg_attr_inert)
    );
    // RED: an `ImplItem::Macro` invocation in an impl body (which can expand to
    // a hidden method / associated item) fires.
    let impl_item_macro = r#"
        mod projector { mod sealed { pub trait Sealed {} } }
        mod carrier {
            mod payload {}
            impl OutputTypeExpr {
                sneaky_method_macro!{}
            }
        }
    "#;
    assert!(
        topo_violations(impl_item_macro)
            .iter()
            .any(|m| m.contains("sneaky_method_macro") && m.contains("ImplItem::Macro")),
        "self-test: an `ImplItem::Macro` invocation in an impl body MUST fire (it can inject a \
         hidden method); got {:?}",
        topo_violations(impl_item_macro)
    );
    // RED: a `TraitItem::Macro` invocation in a trait body fires.
    let trait_item_macro = r#"
        mod projector {
            mod sealed { pub trait Sealed {} }
            trait Decoy { sneaky_trait_macro!{} }
        }
        mod carrier { mod payload {} }
    "#;
    assert!(
        topo_violations(trait_item_macro)
            .iter()
            .any(|m| m.contains("sneaky_trait_macro") && m.contains("TraitItem::Macro")),
        "self-test: a `TraitItem::Macro` invocation in a trait body MUST fire; got {:?}",
        topo_violations(trait_item_macro)
    );
    // RED: a `use …sealed::Sealed as S;` alias of the seal marker fires (it would
    // let `impl S for HotCap {}` evade the last-segment-`Sealed` seal inventory).
    let sealed_alias = r#"
        mod projector {
            mod sealed { pub trait Sealed {} }
            use sealed::Sealed as S;
            impl S for crate::hot::HotCap<'_, '_> {}
        }
        mod carrier { mod payload {} }
    "#;
    assert!(
        topo_violations(sealed_alias)
            .iter()
            .any(|m| m.contains("sealed alias") && m.contains("Sealed as S")),
        "self-test: a `use sealed::Sealed as S;` alias MUST fire (it evades the last-segment-Sealed \
         seal-impl inventory); got {:?}",
        topo_violations(sealed_alias)
    );
    // RED: a `type Inner = TypeExpr;` alias ANYWHERE in the owner file fires
    // (the launder vector a method-return-type recogniser misses — a fn returns
    // `&Inner` not `&TypeExpr`). Here it sits at FILE scope, OUTSIDE the
    // carrier/payload vault the per-vault method scan walks.
    let owner_typeexpr_alias = r#"
        type Inner = verter_type_expr::TypeExpr;
        mod projector { mod sealed { pub trait Sealed {} } }
        mod carrier { mod payload {} }
    "#;
    assert!(
        topo_violations(owner_typeexpr_alias)
            .iter()
            .any(|m| m.contains("typeexpr alias Inner")),
        "self-test: a file-scope `type Inner = TypeExpr;` (outside the vault) MUST fire the \
         owner-wide TypeExpr-alias ban; got {:?}",
        topo_violations(owner_typeexpr_alias)
    );

    // PASS: the known-good vault topology + the sanctioned `macro_rules!`
    // DEFINITION + inert attributes produce ZERO topology violations.
    let good = r#"
        #[cfg(test)]
        struct Whatever;
        mod projector {
            mod sealed { pub trait Sealed {} }
            #[cfg(test)]
            impl OutputProjector for TestOutputCap<'_, '_> {
                fn dispatch(&self) -> &ProjectSemanticDispatch<'_> { self.dispatch }
            }
        }
        mod carrier {
            mod payload {
                #[cfg(any(test, feature = "test-support"))]
                fn x() {}
            }
        }
        macro_rules! define_output_capability { () => {}; }
    "#;
    assert!(
        topo_violations(good).is_empty(),
        "self-test: the known-good vault topology (only projector / projector::sealed / carrier / \
         carrier::payload, the sanctioned macro DEFINITION, inert attrs) MUST pass; got {:?}",
        topo_violations(good)
    );
}

// ===========================================================================
// (2c) [P2-1] `mod sealed` is PRIVATE inside `mod projector` — the structural
// fact that makes the carrier-can't-name-sealed seal COMPILER-enforced.
//
// `mod sealed` MUST carry NO visibility modifier (private), NOT `pub(super)`.
// A private `mod sealed` is nameable ONLY from within `mod projector`, so a
// SIBLING module (`carrier` / `carrier::payload`) — or any other crate module —
// that writes `impl projector::sealed::Sealed for HotCap` is `E0603` (module
// `sealed` is private). That E0603 is the PRIMARY (compiler-enforced) barrier
// against a carrier-side laundered seal impl; the registration-inventory
// topology guard above is the defense-in-depth backstop. A `pub(super) mod
// sealed` would leak the marker to the parent `output_materialization` and ALL
// its descendants (including `carrier`), re-opening the laundering hole — so
// this guard FAILS on any visibility modifier on `mod sealed`.
//
// (An IN-CRATE compile-fail proof cannot be a passing test — a sibling that
// names `sealed` fails to compile the whole crate, not one test. The
// out-of-crate trybuild fixture `output_projector_not_impl_outside_crate.rs`
// proves the cross-crate seal; this structural guard pins the in-crate
// private-`mod sealed` fact the E0603 enforcement rests on. The live E0603 for
// a planted carrier-side `impl projector::sealed::Sealed for HotCap` is in the
// fix-cycle report's D1-honesty section.)
// ===========================================================================

/// The visibility of `mod sealed` inside `mod projector`, or `None` if the
/// nested `projector::sealed` module is not found. Returns the parsed
/// `syn::Visibility` so the guard can assert it is `Inherited` (private).
fn sealed_module_visibility(file: &syn::File) -> Option<syn::Visibility> {
    fn find_in(items: &[syn::Item], parent_is_projector: bool) -> Option<syn::Visibility> {
        for item in items {
            if let syn::Item::Mod(m) = item {
                let name = m.ident.to_string();
                if parent_is_projector && name == "sealed" {
                    return Some(m.vis.clone());
                }
                if let Some((_, inner)) = &m.content {
                    if let Some(v) = find_in(inner, name == "projector") {
                        return Some(v);
                    }
                }
            }
        }
        None
    }
    find_in(&file.items, false)
}

#[test]
fn sealed_module_is_private_not_pub_super() {
    let src = read_rel(OWNER_REL);
    let file = syn::parse_file(&src).expect("parse output_materialization.rs");
    let vis = sealed_module_visibility(&file).unwrap_or_else(|| {
        panic!(
            "[P2-1] anti-vacuity: `mod sealed` was NOT found inside `mod projector` in {OWNER_REL} \
             — the seal module is missing or moved"
        )
    });
    assert!(
        matches!(vis, syn::Visibility::Inherited),
        "[P2-1] STRUCTURAL FENCE: `mod sealed` inside `mod projector` MUST be PRIVATE (no \
         visibility modifier), NOT `{}`. A private `mod sealed` is nameable ONLY from within \
         `projector`, so a sibling `carrier` / `carrier::payload` (or any other crate module) that \
         writes `impl projector::sealed::Sealed for HotCap` is `E0603` (module `sealed` is \
         private) — the COMPILER-enforced barrier against a carrier-side laundered seal. A \
         `pub(super)` here would leak the marker to the parent `output_materialization` and ALL \
         its descendants (including the sibling `carrier`), re-opening the laundering hole the \
         topology guard would then be the SOLE defense against",
        vis.to_token_stream()
    );
}

#[test]
fn sealed_module_privacy_self_test_discriminates() {
    fn vis_of(src: &str) -> Option<syn::Visibility> {
        let file = syn::parse_file(src).expect("parse synthetic");
        sealed_module_visibility(&file)
    }
    // PASS: a PRIVATE `mod sealed` inside `mod projector` (the sanctioned shape)
    // is `Inherited`.
    let private = r#"
        mod projector {
            mod sealed { pub trait Sealed {} }
        }
    "#;
    assert!(
        matches!(vis_of(private), Some(syn::Visibility::Inherited)),
        "self-test: a private `mod sealed` MUST be classified `Inherited`; got {:?}",
        vis_of(private).map(|v| v.to_token_stream().to_string())
    );
    // RED: a `pub(super) mod sealed` (the pre-fix [P2-1] defect — leaks the
    // marker to the sibling `carrier`) is NOT `Inherited`, so the guard's
    // `matches!(…, Inherited)` assertion FAILS.
    let pub_super = r#"
        mod projector {
            pub(super) mod sealed { pub trait Sealed {} }
        }
    "#;
    assert!(
        !matches!(vis_of(pub_super), Some(syn::Visibility::Inherited)),
        "self-test: a `pub(super) mod sealed` MUST NOT be classified private — it is the exact \
         [P2-1] defect (leaks the marker to the sibling carrier); got {:?}",
        vis_of(pub_super).map(|v| v.to_token_stream().to_string())
    );
    // RED: a `pub(crate) mod sealed` (even wider) is likewise not private.
    let pub_crate = r#"
        mod projector {
            pub(crate) mod sealed { pub trait Sealed {} }
        }
    "#;
    assert!(
        !matches!(vis_of(pub_crate), Some(syn::Visibility::Inherited)),
        "self-test: a `pub(crate) mod sealed` MUST NOT be classified private; got {:?}",
        vis_of(pub_crate).map(|v| v.to_token_stream().to_string())
    );
    // A `mod sealed` NOT under `projector` (e.g. a top-level `mod sealed`) is
    // NOT the one this guard pins — the parent-is-projector gate excludes it.
    let wrong_parent = r#"
        mod other {
            mod sealed { pub trait Sealed {} }
        }
    "#;
    assert!(
        vis_of(wrong_parent).is_none(),
        "self-test: a `mod sealed` NOT inside `mod projector` MUST NOT be picked up (the guard \
         pins `projector::sealed` specifically); got {:?}",
        vis_of(wrong_parent).map(|v| v.to_token_stream().to_string())
    );
}

#[test]
fn output_carriers_have_no_inherent_typeexpr_escape_method() {
    let src = read_rel(OWNER_REL);
    let file = syn::parse_file(&src).expect("parse output_materialization.rs");
    // CLOSED ITEM/SIGNATURE ALLOWLIST over the carrier + payload vault: every
    // production `fn` returning `TypeExpr` / `&TypeExpr` MUST be either
    // capability-gated (a `P: OutputProjector` bound) or EXACTLY test-gated.
    // This replaces the old finite name-blacklist (`into_inner` /
    // `as_type_expr` / `as_inner`) — an unlisted name (`raw` / `leak` /
    // `payload`) could evade a blacklist, but ANY un-gated `-> &TypeExpr` fn
    // fires the allowlist regardless of name.
    let escapes = carrier_uncapped_typeexpr_methods(&file);
    assert!(
        escapes.is_empty(),
        "carrier/vault inner-`TypeExpr` accessor violation(s) — a method returning `TypeExpr` / \
         `&TypeExpr` MUST be capability-gated (`P: OutputProjector`) or exactly test-gated; the \
         only sanctioned readers are `into_type_expr` / `type_expr` (cap-gated) + `*_for_test` \
         (test-gated):\n{}",
        escapes.join("\n")
    );
}

#[test]
fn output_carriers_inherent_escape_method_self_test_discriminates() {
    fn escapes(src: &str) -> Vec<String> {
        let file = syn::parse_file(src).expect("parse synthetic");
        carrier_uncapped_typeexpr_methods(&file)
    }
    // RED: a planted `fn leak(&self) -> &TypeExpr` with NO cap param on a vault
    // impl fires — the named-blacklist evasion the allowlist closes.
    let leak = r#"
        mod carrier {
            impl OutputTypeExpr {
                pub(crate) fn leak(&self) -> &TypeExpr { &self.0 }
            }
        }
    "#;
    assert!(
        escapes(leak)
            .iter()
            .any(|m| m.contains("OutputTypeExpr::leak")),
        "self-test: a planted `fn leak(&self) -> &TypeExpr` (no cap, not in any name-blacklist) \
         MUST be caught by the signature allowlist; got {:?}",
        escapes(leak)
    );
    // RED: a `fn raw(self) -> TypeExpr` inside the nested `payload` vault fires.
    let raw = r#"
        mod carrier {
            mod payload {
                impl OutputPayload {
                    pub(super) fn raw(self) -> TypeExpr { self.0 }
                }
            }
        }
    "#;
    assert!(
        escapes(raw)
            .iter()
            .any(|m| m.contains("OutputPayload::raw")),
        "self-test: a planted un-gated `fn raw(self) -> TypeExpr` in the payload vault MUST be \
         caught; got {:?}",
        escapes(raw)
    );
    // RED: even the canonical accessor NAME but with the cap bound REMOVED fires
    // (the allowlist keys on the SIGNATURE, not the name).
    let named_but_uncapped = r#"
        mod carrier {
            impl OutputTypeExpr {
                pub(crate) fn into_type_expr(self) -> TypeExpr { self.0 }
            }
        }
    "#;
    assert!(
        escapes(named_but_uncapped)
            .iter()
            .any(|m| m.contains("OutputTypeExpr::into_type_expr")),
        "self-test: an `into_type_expr` WITHOUT the `P: OutputProjector` cap bound MUST fire (the \
         allowlist is signature-based, not name-based); got {:?}",
        escapes(named_but_uncapped)
    );
    // RED: the ALIAS LAUNDER — `type Inner = TypeExpr; pub(super) fn
    // alias_leak(&self) -> &Inner` — fires on the BANNED alias declaration. The
    // method returns `&Inner` (NOT `&TypeExpr`), so the return-type recogniser
    // alone would MISS it; the vault-scoped `type Inner = TypeExpr` alias ban
    // closes the hole. This MIRRORS the field-privacy guard's alias self-test
    // (`output_carrier_payload_fields_self_test_discriminates`, the `aliased`
    // case) so the inherent-method and field guards are SYMMETRIC on the alias
    // trick.
    let alias_leak = r#"
        mod carrier {
            mod payload {
                type Inner = TypeExpr;
                impl OutputPayload {
                    pub(super) fn alias_leak(&self) -> &Inner { &self.0 }
                }
            }
        }
    "#;
    assert!(
        escapes(alias_leak)
            .iter()
            .any(|m| m.contains("type Inner") && m.contains("aliases `TypeExpr`")),
        "self-test: a `type Inner = TypeExpr` alias in the payload vault MUST be caught at the alias \
         declaration — the alias-launder the return-type recogniser alone misses (a method returns \
         `&Inner`, not `&TypeExpr`); got {:?}",
        escapes(alias_leak)
    );
    // RED: a nested-`TypeExpr` alias (`type Inner = Option<TypeExpr>`) fires too
    // (the RHS recogniser descends groups).
    let alias_nested = r#"
        mod carrier {
            type Boxed = Box<TypeExpr>;
        }
    "#;
    assert!(
        escapes(alias_nested)
            .iter()
            .any(|m| m.contains("type Boxed") && m.contains("aliases `TypeExpr`")),
        "self-test: a `type Boxed = Box<TypeExpr>` nested-mention alias in the vault MUST be caught; \
         got {:?}",
        escapes(alias_nested)
    );
    // PASS: a vault alias that does NOT mention `TypeExpr` (an unrelated alias)
    // does NOT fire — the ban is keyed on a `TypeExpr` RHS, not all aliases.
    let alias_unrelated = r#"
        mod carrier {
            type NodeId = SemanticNodeId;
        }
    "#;
    assert!(
        escapes(alias_unrelated).is_empty(),
        "self-test: a vault alias NOT mentioning `TypeExpr` (e.g. `type NodeId = SemanticNodeId`) \
         MUST NOT fire — only `TypeExpr`-bearing aliases are banned; got {:?}",
        escapes(alias_unrelated)
    );
    // PASS: a `type X = TypeExpr` alias OUTSIDE the vault (not in carrier/payload)
    // does NOT fire — the ban is vault-scoped.
    let alias_outside_vault = r#"
        type Inner = TypeExpr;
        mod projector {
            type AlsoOutside = TypeExpr;
        }
    "#;
    assert!(
        escapes(alias_outside_vault).is_empty(),
        "self-test: a `type X = TypeExpr` alias OUTSIDE the carrier/payload vault MUST NOT fire — \
         the ban is vault-scoped (the topology guard pins the module set); got {:?}",
        escapes(alias_outside_vault)
    );
    // PASS: the sanctioned capability-gated accessors do NOT fire.
    let cap_gated = r#"
        mod carrier {
            impl OutputTypeExpr {
                pub(crate) fn into_type_expr<P: OutputProjector + ?Sized>(self, cap: &P) -> TypeExpr { self.0.into_type_expr(cap) }
            }
            impl MaterializedOutputTypeExpr {
                pub(crate) fn type_expr<P: OutputProjector + ?Sized>(&self, cap: &P) -> &TypeExpr { self.type_expr.0.type_expr(cap) }
            }
        }
    "#;
    assert!(
        escapes(cap_gated).is_empty(),
        "self-test: the capability-gated `into_type_expr` / `type_expr` accessors MUST NOT fire \
         (they carry the `P: OutputProjector` bound); got {:?}",
        escapes(cap_gated)
    );
    // PASS: the EXACTLY test-gated `*_for_test` accessor does NOT fire.
    let test_gated = r#"
        mod carrier {
            mod payload {
                impl OutputPayload {
                    #[cfg(any(test, feature = "test-support"))]
                    pub(super) fn type_expr_for_test(&self) -> &TypeExpr { &self.0 }
                }
            }
        }
    "#;
    assert!(
        escapes(test_gated).is_empty(),
        "self-test: the exactly test-gated `type_expr_for_test` accessor MUST NOT fire; got {:?}",
        escapes(test_gated)
    );
    // RED: a `*_for_test`-shaped accessor with a PRODUCTION-satisfiable gate
    // (`debug_assertions`) fires — it is NOT exactly test-gated and NOT
    // cap-gated.
    let debug_gated = r#"
        mod carrier {
            mod payload {
                impl OutputPayload {
                    #[cfg(any(test, debug_assertions))]
                    pub(super) fn type_expr_for_test(&self) -> &TypeExpr { &self.0 }
                }
            }
        }
    "#;
    assert!(
        escapes(debug_gated)
            .iter()
            .any(|m| m.contains("OutputPayload::type_expr_for_test")),
        "self-test: a `*_for_test` accessor gated `#[cfg(any(test, debug_assertions))]` (debug-\
         reachable) is NEITHER cap-gated NOR exactly test-gated and MUST fire; got {:?}",
        escapes(debug_gated)
    );
}

#[test]
fn output_carrier_payload_fields_are_private() {
    let src = read_rel(OWNER_REL);
    let file = syn::parse_file(&src).expect("parse output_materialization.rs");
    // Every field of the carrier/payload vault structs (`OutputPayload`,
    // `OutputTypeExpr`, `MaterializedOutputTypeExpr`) must be PRIVATE
    // (`Visibility::Inherited`) REGARDLESS of the spelled field type. This
    // catches both a widened `pub`/`pub(crate)` payload field AND the
    // `type Inner = TypeExpr; struct OutputPayload(pub Inner)` alias launder
    // (the vis is read structurally, never keyed on the spelled type name).
    let violations = vault_nonprivate_fields(&file);
    assert!(
        violations.is_empty(),
        "carrier/payload vault field-privacy violation(s):\n{}",
        violations.join("\n")
    );
}

#[test]
fn output_carrier_payload_fields_self_test_discriminates() {
    fn violations(src: &str) -> Vec<String> {
        let file = syn::parse_file(src).expect("parse synthetic");
        vault_nonprivate_fields(&file)
    }
    // RED: a `pub` tuple field on the payload newtype fires.
    let pub_tuple = r#"
        mod carrier { mod payload { pub(super) struct OutputPayload(pub TypeExpr); } }
    "#;
    assert!(
        !violations(pub_tuple).is_empty(),
        "self-test: a `pub` inner payload tuple field MUST be caught"
    );
    // RED: the alias launder — `type Inner = TypeExpr; struct OutputPayload(pub
    // Inner)` — fires too, because the vis is read structurally (NOT keyed on
    // the spelled type name `TypeExpr`).
    let aliased = r#"
        mod carrier { mod payload {
            type Inner = TypeExpr;
            pub(super) struct OutputPayload(pub Inner);
        } }
    "#;
    assert!(
        !violations(aliased).is_empty(),
        "self-test: a `pub` field spelled with a `type Inner = TypeExpr` alias MUST be caught — \
         the vis is read structurally, not by the spelled type name"
    );
    // RED: a `pub(crate)` named payload field on the reduced carrier fires.
    let pub_named = r#"
        mod carrier {
            pub(crate) struct MaterializedOutputTypeExpr {
                pub(crate) type_expr: OutputTypeExpr,
            }
        }
    "#;
    assert!(
        !violations(pub_named).is_empty(),
        "self-test: a `pub(crate)` named payload field MUST be caught"
    );
    // PASS: the known-good fully-private vault fields do NOT fire.
    let good = r#"
        mod carrier {
            mod payload { pub(super) struct OutputPayload(TypeExpr); }
            pub(crate) struct OutputTypeExpr(payload::OutputPayload);
            pub(crate) struct MaterializedOutputTypeExpr {
                node_id: Option<SemanticNodeId>,
                type_expr: OutputTypeExpr,
                dep_signature: DepSignature,
                result_is_partial: bool,
            }
        }
    "#;
    assert!(
        violations(good).is_empty(),
        "self-test: fully-private vault fields MUST pass; got {:?}",
        violations(good)
    );
}

// ===========================================================================
// (3) [P1-A] TERMINAL-SINK capability mint-scope guard (Rust-visibility model).
//
// Compiler privacy DOES enforce that a mint constructor scoped
// `pub(in P)` is uncallable outside `P` and its descendants (proven by the
// planted-exploit E0624 in the report). What the COMPILER cannot express is the
// IDENTITY policy "the modules reachable from a cap's `pub(in P)` mint scope
// must be EXACTLY this cap's true output-SINK module set — no non-sink helper
// descendant, no subtree root that owns non-sink children, no crate root". A
// `pub(in P)` grants the mint to `P` AND EVERY module at-or-under `P`, so the
// danger is not just an over-wide `P` string — it is ANY production module
// reachable from `P` that is not a genuine output sink (e.g. a future
// contributor adding `mod helper;` under the sink module, or re-widening `mint:`
// to a subtree that owns non-sink children). Either re-opens the laundering hole
// the review found while still compiling.
//
// This guard models ACTUAL Rust visibility. For every cap's `mint: pub(in P)`
// it builds the PRODUCTION module tree reachable at `P` and below (walking
// `mod` declarations from `P`'s source file, descending inline + out-of-line
// modules, EXCLUDING `#[cfg(test)]` modules — a test module is not a production
// sink and exercising the cap from tests is sanctioned), then FAILS unless that
// reachable set EXACTLY equals the cap's per-cap `SANCTIONED_SINK_MODULES`
// allowlist. DEFAULT-DENY: a NEW non-sink descendant added later under any mint
// scope FAILS the guard WITHOUT being named in any denylist, because it appears
// in the reachable tree and is not on the allowlist. The by-NAME
// `SANCTIONED_SINK_MODULES` list is the identity residual the compiler cannot
// express — recorded like `SANCTIONED_OUTPUT_CAPS`; the construct-by-kind facts
// (the module-tree edges, the `#[cfg(test)]` classification, set equality) are
// computed structurally from source, not spelled.
// ===========================================================================

/// The sanctioned Kind-B reverse-raise bridge modules — the COMPLETE caller
/// set of `legacy_semantic_type_expr_bridge` (they obtain a bare `TypeExpr`
/// through that interim non-sealed path, NOT a sealed capability). For the
/// fence to hold, NO output cap's `pub(in P)` mint scope `P` may be an
/// ANCESTOR-OR-EQUAL of any of these modules — because `pub(in P)` is callable
/// from `P` AND every module at-or-under `P`, so a bridge module that is a
/// DESCENDANT-OR-EQUAL of a mint scope could call the cap's `new` and launder
/// the carrier. (This is the direction-CORRECT reachability the prior guard
/// inverted.)
///
/// The set is the three modules that actually call the bridge:
/// `meta_resolve::dispatch_helpers` (dispatch_helpers.rs), `host_manage::eval_env`
/// (eval_env.rs), and `project_semantic_dispatch` itself (the `mod.rs` route
/// fixpoint + slot-binding raises). The third was MISSING from the prior set —
/// a `pub(in crate::project_semantic_dispatch)` widening would have slipped.
const KIND_B_BRIDGE_MODULES: &[&str] = &[
    "crate :: meta_resolve :: dispatch_helpers",
    "crate :: host_manage :: eval_env",
    "crate :: project_semantic_dispatch",
];

/// The AUTHORITATIVE per-cap output-SINK-module allowlist: each registered
/// output capability ↦ the EXACT set of PRODUCTION (` :: `-spaced) module paths
/// that constitute that cap's reachable mint scope. For every cap's
/// `mint: pub(in P)`, the production module tree reachable at `P`-and-below
/// (EXCLUDING `#[cfg(test)]` modules) MUST EQUAL this set — no extra reachable
/// module (a non-sink helper descendant, a subtree root owning non-sink
/// children), no missing one (anti-vacuity).
///
/// This is the by-NAME identity residual Rust cannot express (the compiler
/// enforces `pub(in P)` reachability, but not "the modules reachable from `P`
/// are EXACTLY these true output sinks"), hence the guard-local
/// Structural-Confinement record:
///
/// ```text
/// scanner_invariant: output_cap_mint_scope_reaches_only_true_sink_modules
/// scanner_justification: Rust enforces pub(in P) reachability but cannot express "every module reachable from P is a true output SINK"; a non-sink helper descendant under P would compile yet be able to mint+unwrap.
/// mechanism_ruling: structural-confinement-first — the per-sink-module pub(in <terminal>) mint scope is the COMPILER-enforced primary (a non-sink mint is E0624); this by-name sink-module allowlist is the bounded residual the compiler cannot express. The reachable-module tree is computed STRUCTURALLY from source `mod` declarations (construct-by-kind), the `#[cfg(test)]` exclusion is read from attributes, and the verdict is set EQUALITY — default-deny, a new non-sink descendant fails WITHOUT a denylist entry.
/// hardening_rounds: 0
/// hardening_history: replaces the per-leaf string-equality SANCTIONED_CAP_MINT_SCOPES + the direction-correct Kind-B-bridge-ancestor check, which pinned only the mint-scope STRING and so missed a non-sink descendant added UNDER an otherwise-correct leaf scope.
/// ```
///
/// Most caps' reachable scope is a single terminal sink module; two have a
/// genuine sink CHILD (`vue_exec` owns the `normalize` normalizer sink) or are
/// a dedicated terminal sink submodule (`meta_resolve::projectors::output_sink`
/// — extracted exactly so the parent `projectors`' non-sink helpers cannot
/// mint). `component_meta_methods` + `svelte_exec` each own one `#[cfg(test)]`
/// test submodule that is correctly EXCLUDED from the production reachable tree.
///
/// Sorted by cap name so the comparison against the live registry is
/// order-stable.
const SANCTIONED_SINK_MODULES: &[(&str, &[&str])] = &[
    (
        "HostManageComponentMetaOutputCap",
        &["crate :: host_manage :: component_meta_methods"],
    ),
    (
        "MetaQueryRegistryOutputCap",
        &["crate :: resolver_core :: component_meta_query_engine :: registry_decl"],
    ),
    (
        "MetaQuerySurfaceOutputCap",
        &["crate :: resolver_core :: component_meta_query_engine :: surface"],
    ),
    (
        "MetaResolveFieldTypesOutputCap",
        &["crate :: meta_resolve :: materialize :: field_types"],
    ),
    (
        "MetaResolveProjectorsOutputCap",
        &["crate :: meta_resolve :: projectors :: output_sink"],
    ),
    ("TypeinfoRaiseOutputCap", &["crate :: typeinfo :: raise"]),
    (
        "TypeinfoSvelteSurfaceOutputCap",
        &["crate :: typeinfo :: framework_surface :: svelte_exec"],
    ),
    (
        "TypeinfoVueSurfaceOutputCap",
        &[
            "crate :: typeinfo :: framework_surface :: vue_exec",
            "crate :: typeinfo :: framework_surface :: vue_exec :: normalize",
        ],
    ),
];

/// Normalise a `pub(in <path>)` path's token spelling to the canonical
/// `proc_macro2` ` :: `-spaced form so prefix comparisons are spelling-stable.
fn normalize_mod_path(path: &str) -> String {
    // `proc_macro2`'s `TokenStream::to_string` already emits ` :: `-spaced
    // paths; collapse any incidental double spaces for safety.
    path.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Is `ancestor` an ancestor-or-equal module path of `descendant`? Both are
/// ` :: `-spaced canonical paths. `crate :: a` is an ancestor-or-equal of
/// `crate :: a` and `crate :: a :: b`, but NOT of `crate :: ab`.
fn is_mod_ancestor_or_equal(ancestor: &str, descendant: &str) -> bool {
    let a = normalize_mod_path(ancestor);
    let d = normalize_mod_path(descendant);
    d == a || d.starts_with(&format!("{a} :: "))
}

/// Convert a ` :: `-spaced module path (`crate :: a :: b :: c`) to the segment
/// directory prefix used to resolve its source file under `src/` — e.g.
/// `crate :: meta_resolve :: projectors :: output_sink` -> `meta_resolve/projectors/output_sink`
/// (the `crate ::` head is dropped; segments are joined with `/`). The actual
/// file is then this prefix + `.rs` OR this prefix + `/mod.rs`.
fn mod_path_to_rel_prefix(mod_path: &str) -> String {
    normalize_mod_path(mod_path)
        .split(" :: ")
        .skip(1) // drop `crate`
        .collect::<Vec<_>>()
        .join("/")
}

/// A `mod` declaration is a `#[cfg(test)]` (or `#[cfg(any(test, …))]`) test
/// module — read structurally from its attributes so a test submodule is
/// EXCLUDED from the production reachable tree (exercising the cap from tests is
/// sanctioned; a test module is not a production sink).
fn mod_is_cfg_test(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|a| {
        if !a.path().is_ident("cfg") {
            return false;
        }
        // Any `cfg` whose token tree mentions the `test` predicate ident gates
        // the module out of production. (The canonical forms are `cfg(test)` and
        // `cfg(any(test, feature = "test-support"))`; a `cfg(not(test))` would
        // NOT mention a bare `test` predicate as the gate — but the owner sinks
        // never use such a gate on a `mod`, and the conservative direction here
        // is to EXCLUDE anything `test`-gated from the production tree.)
        matches!(&a.meta, syn::Meta::List(list)
            if list.tokens.clone().into_iter().any(|tt| matches!(tt, proc_macro2::TokenTree::Ident(id) if id == "test")))
    })
}

/// The `#[path = "…"]` override on a `mod` declaration, if present (used by the
/// `#[cfg(test)]` test submodules — resolved relative to the parent module's
/// directory). Returns the raw path string.
fn mod_path_attr(attrs: &[syn::Attribute]) -> Option<String> {
    for a in attrs {
        if a.path().is_ident("path") {
            if let syn::Meta::NameValue(nv) = &a.meta {
                if let syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Str(s),
                    ..
                }) = &nv.value
                {
                    return Some(s.value());
                }
            }
        }
    }
    None
}

/// The result of a mint-scope module-tree walk: the set of ` :: `-spaced
/// reachable PRODUCTION module paths, plus any resolution/parse errors (a
/// non-empty error vec means the tree could not be proven — itself a failure).
type ReachableTree = (std::collections::BTreeSet<String>, Vec<String>);

/// Build the PRODUCTION module tree reachable at a cap's `mint: pub(in P)` scope
/// `P` and BELOW, as a set of ` :: `-spaced module paths (including `P` itself).
///
/// Resolves `P`'s source file via `load` (trying `<prefix>.rs` then
/// `<prefix>/mod.rs`), parses it, and walks every `mod` declaration: an inline
/// `mod m { … }` is recorded and descended in place; an out-of-line `mod m;` is
/// recorded and its file resolved relative to the current module's directory
/// (`<dir>/m.rs` or `<dir>/m/mod.rs`) and recursed. `#[cfg(test)]` modules are
/// SKIPPED entirely (not recorded, not descended). `load(rel)` returns the file
/// source for a crate-relative path (real = read from disk; the self-test feeds
/// a synthetic map), or `None` if the file does not exist.
///
/// Returns `(reachable_module_paths, errors)`; a non-empty `errors` (e.g. the
/// root file or an out-of-line submodule file could not be resolved) is itself a
/// guard failure — the tree could not be proven.
fn reachable_production_modules(
    mint_scope: &str,
    load: &dyn Fn(&str) -> Option<String>,
) -> ReachableTree {
    let mut reachable = std::collections::BTreeSet::new();
    let mut errors = Vec::new();
    let root_prefix = mod_path_to_rel_prefix(mint_scope);

    // Resolve a module-file rel-prefix to the (rel_file, dir_prefix) that
    // exists: `<prefix>.rs` (dir = `<prefix>`) or `<prefix>/mod.rs` (dir =
    // `<prefix>`). The directory for a module's CHILDREN is `<prefix>` in both
    // cases (a sibling dir named after a `foo.rs` file, OR the `foo/` dir of a
    // `foo/mod.rs`).
    fn resolve(
        rel_prefix: &str,
        load: &dyn Fn(&str) -> Option<String>,
    ) -> Option<(String, String)> {
        let file_rs = format!("src/{rel_prefix}.rs");
        if let Some(src) = load(&file_rs) {
            return Some((src, rel_prefix.to_string()));
        }
        let file_mod = format!("src/{rel_prefix}/mod.rs");
        if load(&file_mod).is_some() {
            // re-load to return the source (cheap; loader is a closure)
            return load(&file_mod).map(|src| (src, rel_prefix.to_string()));
        }
        None
    }

    fn walk(
        mod_path: &str,
        rel_prefix: &str,
        load: &dyn Fn(&str) -> Option<String>,
        reachable: &mut std::collections::BTreeSet<String>,
        errors: &mut Vec<String>,
    ) {
        reachable.insert(mod_path.to_string());
        let Some((src, dir_prefix)) = resolve(rel_prefix, load) else {
            errors.push(format!(
                "could not resolve a source file for module `{mod_path}` (tried \
                 `src/{rel_prefix}.rs` and `src/{rel_prefix}/mod.rs`) — the mint-scope module \
                 tree could not be built"
            ));
            return;
        };
        let file = match syn::parse_file(&src) {
            Ok(f) => f,
            Err(e) => {
                errors.push(format!("parse `{mod_path}` ({rel_prefix}): {e}"));
                return;
            }
        };
        descend_items(&file.items, mod_path, &dir_prefix, load, reachable, errors);
    }

    /// Walk a slice of items at module `mod_path` (whose child files resolve
    /// under `dir_prefix/`), recording + descending non-test `mod` decls.
    fn descend_items(
        items: &[syn::Item],
        mod_path: &str,
        dir_prefix: &str,
        load: &dyn Fn(&str) -> Option<String>,
        reachable: &mut std::collections::BTreeSet<String>,
        errors: &mut Vec<String>,
    ) {
        for item in items {
            let syn::Item::Mod(m) = item else { continue };
            if mod_is_cfg_test(&m.attrs) {
                continue; // test submodule — not a production sink
            }
            let child_name = m.ident.to_string();
            let child_path = format!("{mod_path} :: {child_name}");
            match &m.content {
                Some((_, inner)) => {
                    // Inline module: record + descend in place. Its OWN children
                    // resolve under `dir_prefix/child_name/` (if any further
                    // out-of-line decls appear nested — rare, but handled).
                    reachable.insert(child_path.clone());
                    let child_dir = format!("{dir_prefix}/{child_name}");
                    descend_items(inner, &child_path, &child_dir, load, reachable, errors);
                }
                None => {
                    // Out-of-line `mod child;` — resolve relative to the parent
                    // module's directory. A `#[path = "rel.rs"]` override
                    // resolves relative to `dir_prefix` too.
                    let child_rel_prefix = match mod_path_attr(&m.attrs) {
                        Some(p) => {
                            // `#[path = "foo.rs"]` → strip a trailing `.rs` so the
                            // resolver re-adds it; a `#[path = "foo/mod.rs"]` →
                            // strip `/mod.rs`. Resolve relative to `dir_prefix`.
                            let stripped = p
                                .strip_suffix("/mod.rs")
                                .or_else(|| p.strip_suffix(".rs"))
                                .unwrap_or(&p);
                            format!("{dir_prefix}/{stripped}")
                        }
                        None => format!("{dir_prefix}/{child_name}"),
                    };
                    walk(&child_path, &child_rel_prefix, load, reachable, errors);
                }
            }
        }
    }

    // Entry: resolve + walk the root mint-scope module.
    walk(mint_scope, &root_prefix, load, &mut reachable, &mut errors);
    (reachable, errors)
}

/// The real on-disk loader: a crate-relative path -> file source, or `None` if
/// the file does not exist. Backs [`reachable_production_modules`] for the live
/// guard (the self-test injects a synthetic loader instead).
fn disk_loader(rel: &str) -> Option<String> {
    std::fs::read_to_string(crate_root().join(rel)).ok()
}

/// One `define_output_capability!` invocation's extracted `(cap_name,
/// mint_scope_path)`. `mint_scope_path` is the ` :: `-spaced path inside
/// `pub(in <path>)`, or `None` if the mint visibility is NOT a `pub(in …)`
/// form (which is itself a violation — a wider `pub(crate)` / `pub` mint).
struct CapMintScope {
    file: String,
    cap_name: String,
    /// `Some(path)` for `mint: pub(in <path>)`; `None` for any other mint
    /// visibility (a violation).
    mint_in_path: Option<String>,
}

/// Parse every `define_output_capability! { … ; mint: <vis> }` invocation in
/// the production source and extract `(cap_name, mint_in_path)`. The macro
/// body is `$(#[$meta])* $vis:vis struct $name:ident; mint: $mint_vis:vis` —
/// we read the `struct <Name>;` ident and the `mint:` visibility token run.
fn collect_output_cap_mint_scopes() -> Vec<CapMintScope> {
    use proc_macro2::TokenTree;
    let mut out = Vec::new();
    for (rel, src) in production_src_files() {
        let file = match syn::parse_file(&src) {
            Ok(f) => f,
            Err(_) => continue,
        };
        // The macro is invoked at item position
        // (`<path>::define_output_capability! { … }`). Find each macro item
        // whose path's last segment is `define_output_capability`.
        for item in &file.items {
            let syn::Item::Macro(m) = item else { continue };
            if m.mac
                .path
                .segments
                .last()
                .map(|s| s.ident != "define_output_capability")
                .unwrap_or(true)
            {
                continue;
            }
            let toks: Vec<TokenTree> = m.mac.tokens.clone().into_iter().collect();
            // Find `struct <Name> ;` → the cap name is the ident right after
            // the `struct` keyword.
            let mut cap_name: Option<String> = None;
            for w in toks.windows(2) {
                if let (TokenTree::Ident(kw), TokenTree::Ident(name)) = (&w[0], &w[1]) {
                    if kw == "struct" {
                        cap_name = Some(name.to_string());
                        break;
                    }
                }
            }
            let Some(cap_name) = cap_name else { continue };
            // Find the `mint` ident, then `:`, then the visibility token run
            // up to end of stream. We only need to recognise
            // `pub ( in <path> )` and capture `<path>`.
            let mut mint_in_path: Option<String> = None;
            let mut saw_mint = false;
            let mut i = 0;
            while i < toks.len() {
                if let TokenTree::Ident(id) = &toks[i] {
                    if id == "mint" {
                        saw_mint = true;
                        // After `mint :` expect `pub ( in <path> )`.
                        // Scan forward for the next `pub` then its group.
                        let mut j = i + 1;
                        // skip the `:` punct
                        while j < toks.len() {
                            if let TokenTree::Ident(p) = &toks[j] {
                                if p == "pub" {
                                    // Next token should be a parenthesised group `(in <path>)`.
                                    if let Some(TokenTree::Group(g)) = toks.get(j + 1) {
                                        let inner: Vec<TokenTree> =
                                            g.stream().into_iter().collect();
                                        // inner = `in <path...>`
                                        if let Some(TokenTree::Ident(kw)) = inner.first() {
                                            if kw == "in" {
                                                let path_toks: proc_macro2::TokenStream =
                                                    inner[1..].iter().cloned().collect();
                                                mint_in_path = Some(normalize_mod_path(
                                                    &path_toks.to_string(),
                                                ));
                                            }
                                        }
                                    }
                                    break;
                                }
                            }
                            j += 1;
                        }
                        break;
                    }
                }
                i += 1;
            }
            // `saw_mint` with no captured `pub(in …)` ⇒ a non-`pub(in)` mint
            // visibility (e.g. bare `pub(crate)`), recorded as `None` so the
            // assertion flags it.
            let _ = saw_mint;
            out.push(CapMintScope {
                file: rel.clone(),
                cap_name,
                mint_in_path,
            });
        }
    }
    out
}

/// The per-scope verdict the [`output_cap_mint_scope_is_per_leaf_not_subtree`]
/// guard renders — the POLICY half, parameterised over a `reachable` closure
/// (`mint_scope -> (reachable production module set, errors)`) so the
/// discriminating self-test can feed it SYNTHETIC module trees and observe it
/// FIRE. Returns one message per violation; empty ⇒ every cap's mint scope
/// reaches EXACTLY its true output-SINK module set.
///
/// Checks: (a) the mint visibility is `pub(in <path>)` (a wider `pub(crate)` /
/// `pub` is recorded `None` and FIRES); (b) the cap is on the
/// `SANCTIONED_SINK_MODULES` allowlist; (c) the production module tree reachable
/// at the mint scope (from `reachable`, EXCLUDING `#[cfg(test)]` modules) EXACTLY
/// equals the cap's sink allowlist — a reachable module NOT on the allowlist
/// (default-deny: a new non-sink descendant) FIRES, and an allowlisted module
/// MISSING from the reachable tree (anti-vacuity) FIRES; (d) the tree-build
/// surfaced no resolution error; (e) the observed registered-cap set EXACTLY
/// equals the allowlist (anti-vacuity for a disappeared cap).
fn mint_scope_violations_from_trees(
    scopes: &[CapMintScope],
    reachable: &dyn Fn(&str) -> ReachableTree,
) -> Vec<String> {
    let mut violations: Vec<String> = Vec::new();

    let allow: BTreeMap<&str, &[&str]> = SANCTIONED_SINK_MODULES.iter().copied().collect();

    for s in scopes {
        // (a) The mint visibility MUST be `pub(in <path>)`.
        let Some(path) = &s.mint_in_path else {
            violations.push(format!(
                "`{}` ({}) has a `mint:` visibility that is NOT `pub(in <sink-module>)` — an output \
                 cap MUST be minted with a per-sink-module `pub(in …)` scope, never a wider \
                 `pub(crate)` / `pub`",
                s.cap_name, s.file
            ));
            continue;
        };
        let mint_scope = normalize_mod_path(path);

        // (b) The cap MUST be on the by-name sink-module allowlist.
        let Some(sanctioned) = allow.get(s.cap_name.as_str()) else {
            violations.push(format!(
                "`{}` ({}) is a registered output cap NOT on the sanctioned sink-module allowlist — \
                 a new output sink must be added to `SANCTIONED_SINK_MODULES` (with the EXACT set of \
                 production module paths reachable from its `pub(in …)` mint scope) deliberately, \
                 never silently",
                s.cap_name, s.file
            ));
            continue;
        };
        let sanctioned_set: std::collections::BTreeSet<String> =
            sanctioned.iter().map(|m| normalize_mod_path(m)).collect();

        // (c)+(d) Build the production module tree reachable at the mint scope
        // and require it to EQUAL the cap's sink allowlist.
        let (reachable_set, errors) = reachable(&mint_scope);
        for e in errors {
            violations.push(format!(
                "`{}` ({}) mint-scope tree-build error: {e}",
                s.cap_name, s.file
            ));
        }
        // Default-deny: any reachable production module NOT on the allowlist is
        // a non-sink descendant that can mint the cap.
        for reached in reachable_set.difference(&sanctioned_set) {
            violations.push(format!(
                "`{}` ({}) mints at `pub(in {mint_scope})`, whose reachable PRODUCTION module tree \
                 includes `{reached}` — that module is NOT a sanctioned output SINK for this cap \
                 (`pub(in {mint_scope})` is callable from `{reached}`, so it could mint the cap and \
                 launder a carrier). Either it is a non-sink helper that must move OUT of the mint \
                 scope, or it is a genuine new sink that must be added to this cap's \
                 `SANCTIONED_SINK_MODULES` row deliberately",
                s.cap_name, s.file
            ));
        }
        // Anti-vacuity: an allowlisted sink module that is NOT reachable means
        // the sink moved/was deleted (or the tree walk regressed) — not a pass.
        for missing in sanctioned_set.difference(&reachable_set) {
            violations.push(format!(
                "`{}` ({}) sanctioned sink module `{missing}` is NOT reachable from its `pub(in \
                 {mint_scope})` mint scope — the sink moved/was deleted, or the module-tree walk \
                 regressed (anti-vacuity)",
                s.cap_name, s.file
            ));
        }
    }

    // (e) The observed registered-cap set must EXACTLY equal the allowlist —
    // a sanctioned cap that DISAPPEARED (anti-vacuity) is also a violation.
    let observed: std::collections::BTreeSet<String> =
        scopes.iter().map(|s| s.cap_name.clone()).collect();
    let expected_names: std::collections::BTreeSet<String> =
        allow.keys().map(|k| k.to_string()).collect();
    for missing in expected_names.difference(&observed) {
        violations.push(format!(
            "allowlisted output cap `{missing}` was NOT observed as a `define_output_capability!` \
             invocation in production source — it was deleted/renamed without updating \
             `SANCTIONED_SINK_MODULES`, or the parser regressed (anti-vacuity)"
        ));
    }

    violations
}

/// The LIVE mint-scope verdict: [`mint_scope_violations_from_trees`] wired to
/// the real on-disk module-tree walker ([`reachable_production_modules`] over
/// [`disk_loader`]).
fn mint_scope_violations(scopes: &[CapMintScope]) -> Vec<String> {
    mint_scope_violations_from_trees(scopes, &|mint_scope| {
        reachable_production_modules(mint_scope, &disk_loader)
    })
}

#[test]
fn output_cap_mint_scope_is_per_leaf_not_subtree() {
    let scopes = collect_output_cap_mint_scopes();
    assert!(
        !scopes.is_empty(),
        "expected to find `define_output_capability!` invocations in production source; found none \
         — the parser or the macro spelling changed"
    );
    let violations = mint_scope_violations(&scopes);
    assert!(
        violations.is_empty(),
        "TERMINAL-SINK output-cap mint-scope violation(s) — a cap's `pub(in P)` mint scope reaches \
         a PRODUCTION module that is not a true output sink (the in-subtree laundering hole the \
         compiler privacy does NOT police — `pub(in P)` grants the mint to every module at-or-\
         under P):\n{}",
        violations.join("\n")
    );
}

/// Synthetic `CapMintScope` builder for the self-tests.
fn synthetic_scope(cap: &str, mint: Option<&str>) -> CapMintScope {
    CapMintScope {
        file: "synthetic".to_string(),
        cap_name: cap.to_string(),
        mint_in_path: mint.map(normalize_mod_path),
    }
}

/// A synthetic `reachable` closure backed by a fixed `mint_scope -> tree` map.
/// Used to drive [`mint_scope_violations_from_trees`] in the policy self-test
/// without touching the filesystem. An unmapped scope yields an empty tree with
/// a "no synthetic tree" error (so a mis-set fixture is loud, not vacuous).
fn synthetic_reachable<'a>(
    trees: &'a BTreeMap<String, Vec<&'static str>>,
) -> impl Fn(&str) -> ReachableTree + 'a {
    move |mint_scope: &str| match trees.get(mint_scope) {
        Some(mods) => (
            mods.iter().map(|m| normalize_mod_path(m)).collect(),
            Vec::new(),
        ),
        None => (
            std::collections::BTreeSet::new(),
            vec![format!("no synthetic tree for `{mint_scope}`")],
        ),
    }
}

#[test]
fn output_cap_mint_scope_self_test_discriminates() {
    // The exact sanctioned scope set (the live production cap names + their
    // sink-module mint scopes), each mapped to its EXACT sanctioned sink tree.
    // This MUST PASS (no violations) — the known-good shape.
    let sanctioned: Vec<CapMintScope> = SANCTIONED_SINK_MODULES
        .iter()
        .map(|(cap, mods)| synthetic_scope(cap, Some(mods[0]))) // mint scope = the FIRST sink (the scope root)
        .collect();
    let good_trees: BTreeMap<String, Vec<&'static str>> = SANCTIONED_SINK_MODULES
        .iter()
        .map(|(_cap, mods)| (normalize_mod_path(mods[0]), mods.to_vec()))
        .collect();
    let good = synthetic_reachable(&good_trees);
    assert!(
        mint_scope_violations_from_trees(&sanctioned, &good).is_empty(),
        "self-test: the EXACT sanctioned sink-module scope set + trees MUST pass with no violation; \
         got: {:?}",
        mint_scope_violations_from_trees(&sanctioned, &good)
    );

    // FIRE (RED) — the PRIMARY Q1 defense: a NON-SINK DESCENDANT added under a
    // cap's mint scope. The projectors cap mints at `…::projectors::output_sink`;
    // a future `mod sneaky_helper;` under `output_sink` appears in the reachable
    // PRODUCTION tree and is NOT a sanctioned sink → default-deny FIRES WITHOUT a
    // denylist entry (this is the in-subtree laundering hole the old string-only
    // guard was blind to).
    let mut bad = good_trees.clone();
    bad.insert(
        "crate :: meta_resolve :: projectors :: output_sink".to_string(),
        vec![
            "crate :: meta_resolve :: projectors :: output_sink",
            "crate :: meta_resolve :: projectors :: output_sink :: sneaky_helper",
        ],
    );
    let v = mint_scope_violations_from_trees(&sanctioned, &synthetic_reachable(&bad));
    assert!(
        v.iter()
            .any(|m| m.contains("MetaResolveProjectorsOutputCap")
                && m.contains("output_sink :: sneaky_helper")
                && m.contains("NOT a sanctioned output SINK")),
        "self-test: a NON-SINK descendant `output_sink::sneaky_helper` reachable from the mint \
         scope MUST FIRE the default-deny reachable-tree check; got: {v:?}"
    );

    // FIRE (RED): a cap WIDENED so its mint scope reaches a Kind-B bridge module
    // (the projectors cap re-widened to the `meta_resolve` subtree root, whose
    // reachable tree includes the Kind-B bridge `meta_resolve::dispatch_helpers`
    // — a non-sink module). The reachable-tree check FIRES on the bridge module.
    let widened: Vec<CapMintScope> = sanctioned
        .iter()
        .map(|s| {
            if s.cap_name == "MetaResolveProjectorsOutputCap" {
                synthetic_scope(&s.cap_name, Some("crate :: meta_resolve"))
            } else {
                CapMintScope {
                    file: s.file.clone(),
                    cap_name: s.cap_name.clone(),
                    mint_in_path: s.mint_in_path.clone(),
                }
            }
        })
        .collect();
    let mut widened_trees = good_trees.clone();
    widened_trees.insert(
        "crate :: meta_resolve".to_string(),
        vec![
            "crate :: meta_resolve",
            "crate :: meta_resolve :: projectors :: output_sink",
            KIND_B_BRIDGE_MODULES[0], // crate :: meta_resolve :: dispatch_helpers
        ],
    );
    let v = mint_scope_violations_from_trees(&widened, &synthetic_reachable(&widened_trees));
    assert!(
        v.iter()
            .any(|m| m.contains("MetaResolveProjectorsOutputCap")
                && m.contains("crate :: meta_resolve :: dispatch_helpers")
                && m.contains("NOT a sanctioned output SINK")),
        "self-test: a mint scope whose reachable tree includes the Kind-B bridge \
         `meta_resolve::dispatch_helpers` MUST FIRE the reachable-tree check; got: {v:?}"
    );

    // FIRE (RED): an allowlisted sink module that is NOT reachable (anti-vacuity)
    // — the vue cap's `normalize` sink dropped from the reachable tree.
    let mut missing_trees = good_trees.clone();
    missing_trees.insert(
        "crate :: typeinfo :: framework_surface :: vue_exec".to_string(),
        vec!["crate :: typeinfo :: framework_surface :: vue_exec"], // normalize MISSING
    );
    let v = mint_scope_violations_from_trees(&sanctioned, &synthetic_reachable(&missing_trees));
    assert!(
        v.iter().any(|m| m.contains("TypeinfoVueSurfaceOutputCap")
            && m.contains("vue_exec :: normalize")
            && m.contains("NOT reachable")),
        "self-test: an allowlisted sink module missing from the reachable tree MUST FIRE \
         anti-vacuity; got: {v:?}"
    );

    // FIRE (RED): a non-`pub(in)` mint (recorded as `None`) FIRES.
    let none_scopes: Vec<CapMintScope> = sanctioned
        .iter()
        .map(|s| CapMintScope {
            file: s.file.clone(),
            cap_name: s.cap_name.clone(),
            mint_in_path: if s.cap_name == "MetaResolveProjectorsOutputCap" {
                None
            } else {
                s.mint_in_path.clone()
            },
        })
        .collect();
    let v = mint_scope_violations_from_trees(&none_scopes, &good);
    assert!(
        v.iter()
            .any(|m| m.contains("MetaResolveProjectorsOutputCap") && m.contains("NOT `pub(in")),
        "self-test: a non-`pub(in …)` mint (e.g. bare `pub(crate)`) MUST FIRE; got: {v:?}"
    );

    // FIRE (RED): a DISAPPEARED sanctioned cap (anti-vacuity) — drop one cap from
    // the observed set; the registered-cap-set difference check FIRES.
    let dropped: Vec<CapMintScope> = sanctioned
        .iter()
        .filter(|s| s.cap_name != "TypeinfoRaiseOutputCap")
        .map(|s| CapMintScope {
            file: s.file.clone(),
            cap_name: s.cap_name.clone(),
            mint_in_path: s.mint_in_path.clone(),
        })
        .collect();
    let v = mint_scope_violations_from_trees(&dropped, &good);
    assert!(
        v.iter()
            .any(|m| m.contains("TypeinfoRaiseOutputCap") && m.contains("NOT observed")),
        "self-test: a DISAPPEARED sanctioned cap MUST FIRE the anti-vacuity difference check; \
         got: {v:?}"
    );

    // FIRE (RED): an UNKNOWN cap not on the sink allowlist.
    let unknown = vec![synthetic_scope(
        "RogueOutputCap",
        Some("crate :: rogue :: leaf"),
    )];
    let v = mint_scope_violations_from_trees(&unknown, &good);
    assert!(
        v.iter()
            .any(|m| m.contains("RogueOutputCap") && m.contains("NOT on the sanctioned")),
        "self-test: an unknown cap not on `SANCTIONED_SINK_MODULES` MUST FIRE; got: {v:?}"
    );

    // The ancestor predicate boundary must not false-positive: `crate::a` is
    // NOT an ancestor of `crate::ab` (still used by the reachable-tree builder's
    // out-of-line child resolution + relied on by the doc cross-reference).
    assert!(
        !is_mod_ancestor_or_equal("crate :: a", "crate :: ab"),
        "self-test: `crate::a` MUST NOT be treated as an ancestor of `crate::ab` (prefix-but-not-\
         path-segment)"
    );
}

/// The MODULE-TREE WALKER self-test: drives [`reachable_production_modules`]
/// against SYNTHETIC in-memory source (a fixed `rel-file -> source` map) and
/// proves it (a) records inline + out-of-line submodules, (b) EXCLUDES
/// `#[cfg(test)]` modules, (c) resolves an out-of-line child relative to the
/// parent module's directory, and (d) errors on an unresolvable child. The real
/// guard runs the same walker over the on-disk loader, so this pins the walk's
/// behaviour the live guard depends on.
#[test]
fn mint_scope_module_tree_walker_self_test_discriminates() {
    // Synthetic crate fragment:
    //   src/sink/mod.rs        (the mint-scope root, a dir-module)
    //     mod normalize;       (out-of-line sink child)         -> src/sink/normalize.rs
    //     #[cfg(test)] mod t;  (test submodule — EXCLUDED)      -> (never resolved)
    //     mod inline_helper {} (INLINE non-sink helper)         -> recorded in place
    //   src/sink/normalize.rs  (leaf, no children)
    let map: BTreeMap<&str, &str> = BTreeMap::from([
        (
            "src/sink/mod.rs",
            r#"
                mod normalize;
                #[cfg(test)]
                mod t;
                mod inline_helper {
                    // a nested non-sink helper — recorded as reachable
                }
            "#,
        ),
        ("src/sink/normalize.rs", "// leaf sink, no children\n"),
    ]);
    let loader = |rel: &str| map.get(rel).map(|s| s.to_string());

    let (reachable, errors) = reachable_production_modules("crate :: sink", &loader);
    assert!(
        errors.is_empty(),
        "walker self-test: the synthetic tree must resolve with no errors; got: {errors:?}"
    );
    // The root + the out-of-line sink child + the inline helper are reachable;
    // the `#[cfg(test)] mod t` is EXCLUDED.
    assert!(
        reachable.contains("crate :: sink"),
        "walker: the mint-scope root module must be in the reachable set; got: {reachable:?}"
    );
    assert!(
        reachable.contains("crate :: sink :: normalize"),
        "walker: an out-of-line `mod normalize;` child (resolved to `src/sink/normalize.rs`) must \
         be reachable; got: {reachable:?}"
    );
    assert!(
        reachable.contains("crate :: sink :: inline_helper"),
        "walker: an INLINE `mod inline_helper {{}}` must be reachable (default-deny would then FIRE \
         on it as a non-sink); got: {reachable:?}"
    );
    assert!(
        !reachable.contains("crate :: sink :: t"),
        "walker: a `#[cfg(test)] mod t;` MUST be EXCLUDED from the production reachable tree; got: \
         {reachable:?}"
    );

    // (d) An out-of-line child whose file is MISSING surfaces an error (the tree
    // could not be proven — itself a guard failure in the policy half).
    let map_missing: BTreeMap<&str, &str> = BTreeMap::from([("src/sink/mod.rs", "mod gone;\n")]);
    let loader_missing = |rel: &str| map_missing.get(rel).map(|s| s.to_string());
    let (_r, errors) = reachable_production_modules("crate :: sink", &loader_missing);
    assert!(
        errors.iter().any(|e| e.contains("crate :: sink :: gone")),
        "walker: an out-of-line child with no resolvable file MUST surface an error; got: {errors:?}"
    );

    // The real production trees the live guard relies on: vue_exec → {vue_exec,
    // normalize}; projectors output_sink → {output_sink} only. (Discriminating:
    // a regression that stopped excluding `#[cfg(test)]` or stopped resolving the
    // out-of-line `normalize` child would change these.)
    let (vue, vue_errs) = reachable_production_modules(
        "crate :: typeinfo :: framework_surface :: vue_exec",
        &disk_loader,
    );
    assert!(vue_errs.is_empty(), "vue_exec tree errors: {vue_errs:?}");
    let vue_expected: std::collections::BTreeSet<String> = [
        "crate :: typeinfo :: framework_surface :: vue_exec",
        "crate :: typeinfo :: framework_surface :: vue_exec :: normalize",
    ]
    .iter()
    .map(|m| normalize_mod_path(m))
    .collect();
    assert_eq!(
        vue, vue_expected,
        "the live vue_exec mint-scope reachable PRODUCTION tree must be EXACTLY {{vue_exec, \
         normalize}}"
    );

    let (sink, sink_errs) = reachable_production_modules(
        "crate :: meta_resolve :: projectors :: output_sink",
        &disk_loader,
    );
    assert!(
        sink_errs.is_empty(),
        "output_sink tree errors: {sink_errs:?}"
    );
    let sink_expected: std::collections::BTreeSet<String> =
        ["crate :: meta_resolve :: projectors :: output_sink"]
            .iter()
            .map(|m| normalize_mod_path(m))
            .collect();
    assert_eq!(
        sink, sink_expected,
        "the live projectors output_sink mint-scope reachable PRODUCTION tree must be EXACTLY \
         {{output_sink}} (no non-sink helper descendant)"
    );
}

// ===========================================================================
// (4) [P1-B] carrier `_for_test` accessor gate inventory.
//
// The carrier `from_type_expr_for_test` / `type_expr_for_test` accessors take
// NO capability, so they MUST be gated test-only — reachable from genuine test
// code (in-crate `#[cfg(test)]` + the production-unreachable `test-support`
// feature for the integration binary) but COMPILE-ABSENT from every production
// build (proven by the DEBUG planted-exploit E0599). `#[cfg(any(test,
// debug_assertions))]` would re-open the debug-build hole the review found:
// `debug_assertions` is ON in ordinary debug builds. This guard reads the
// owner module and asserts every carrier `*_for_test` inherent fn is gated
// EXACTLY `cfg(test)` or `cfg(any(test, feature = "test-support"))` — and
// BANS any `debug_assertions`-bearing (or otherwise production-satisfiable)
// gate.
// ===========================================================================

/// Whether a carrier accessor's attribute set is the genuinely test-only
/// (production-unreachable) gate: EXACTLY one `cfg(...)` attribute, and that
/// cfg is EXACTLY `cfg(test)` or `cfg(any(test, feature = "test-support"))`.
///
/// This delegates to the ONE rigorous parsed recogniser
/// [`cfg_is_exactly_test_or_test_support`] (shared from
/// `handle_capable_consumer_guards`) — it is token-tree parsed, NOT
/// substring-matched, so a disjunction carrying ANY extra production-satisfiable
/// arm (`unix`, `debug_assertions`, another `feature`) is REJECTED, while a
/// reordered-but-valid `any(feature = "test-support", test)` is accepted. The
/// prior substring matcher wrongly PASSED `any(test, feature = "test-support",
/// unix)` (production-visible on Unix) and `any(test, feature = "test-support",
/// feature = "prod")`.
fn carrier_for_test_gate_is_sanctioned(attrs: &[syn::Attribute]) -> bool {
    let cfgs: Vec<&syn::Attribute> = attrs.iter().filter(|a| a.path().is_ident("cfg")).collect();
    // Accept iff EXACTLY one `cfg(...)` is present and it is one of the two
    // canonical narrow production-unreachable gates.
    if cfgs.len() != 1 {
        return false;
    }
    let syn::Meta::List(list) = &cfgs[0].meta else {
        // A bare `#[cfg]` path with no predicate list is not a valid gate.
        return false;
    };
    cfg_is_exactly_test_or_test_support(list.tokens.clone())
}

/// Collect every inherent fn on a carrier struct whose name ends `_for_test`,
/// with its parsed attributes (so the rigorous cfg recogniser can inspect the
/// `cfg(...)` predicate's token tree directly — never a substring of the
/// rendered spelling). Walks recursively into the inline `carrier` vault module
/// where the carrier inherent impls now live (the payload-vault restructure
/// moved the carriers from the top-level owner module into `mod carrier`; the
/// gate INVARIANT is unchanged, only the walk descends one level).
fn carrier_for_test_accessor_gates(src: &str) -> Vec<(String, Vec<syn::Attribute>)> {
    const CARRIERS: &[&str] = &["OutputTypeExpr", "MaterializedOutputTypeExpr"];
    let file = syn::parse_file(src).expect("parse owner src");
    fn walk(items: &[syn::Item], out: &mut Vec<(String, Vec<syn::Attribute>)>) {
        for item in items {
            match item {
                syn::Item::Impl(imp) => {
                    if imp.trait_.is_some() {
                        continue; // inherent impls only
                    }
                    let Some(self_name) = impl_self_ty_last_ident(&imp.self_ty) else {
                        continue;
                    };
                    if !CARRIERS.contains(&self_name.as_str()) {
                        continue;
                    }
                    for ii in &imp.items {
                        if let syn::ImplItem::Fn(f) = ii {
                            let fname = f.sig.ident.to_string();
                            if fname.ends_with("_for_test") {
                                out.push((fname, f.attrs.clone()));
                            }
                        }
                    }
                }
                syn::Item::Mod(syn::ItemMod {
                    content: Some((_, inner)),
                    ..
                }) => walk(inner, out),
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    walk(&file.items, &mut out);
    out
}

#[test]
fn carrier_for_test_accessors_are_test_support_gated_not_debug_assertions() {
    let src = read_rel(OWNER_REL);
    let accessors = carrier_for_test_accessor_gates(&src);
    assert!(
        !accessors.is_empty(),
        "expected to find carrier `*_for_test` accessors in {OWNER_REL}; found none — the accessor \
         names or the carrier impl shape changed"
    );
    let mut violations: Vec<String> = Vec::new();
    for (name, attrs) in &accessors {
        if !carrier_for_test_gate_is_sanctioned(attrs) {
            // Render only the `cfg(...)` attrs (skip doc comments) so the
            // failure message points at the gate, not the prose.
            let rendered: String = attrs
                .iter()
                .filter(|a| a.path().is_ident("cfg"))
                .map(|a| a.to_token_stream().to_string())
                .collect::<Vec<_>>()
                .join(" ");
            let rendered = if rendered.is_empty() {
                "<no cfg attribute>".to_string()
            } else {
                rendered
            };
            violations.push(format!(
                "carrier accessor `{name}` is gated `{rendered}` — a capability-free carrier \
                 `_for_test` accessor MUST be gated EXACTLY `#[cfg(test)]` or `#[cfg(any(test, \
                 feature = \"test-support\"))]` (production-unreachable), NEVER `debug_assertions` \
                 (ON in ordinary debug builds) or any other production-satisfiable cfg"
            ));
        }
    }
    assert!(
        violations.is_empty(),
        "carrier `_for_test` accessor gate violation(s) — these would re-open the carrier-unwrap \
         laundering surface in debug builds:\n{}",
        violations.join("\n")
    );
}

#[test]
fn carrier_for_test_gate_self_test_discriminates() {
    // Parse the attributes off a synthetic fn so the recogniser inspects the
    // REAL `cfg(...)` token tree (never a hand-typed rendered string). `body`
    // is the full attribute list, e.g. `#[cfg(any(test, debug_assertions))]`.
    fn attrs_from(body: &str) -> Vec<syn::Attribute> {
        let file: syn::File = syn::parse_str(&format!("{body}\nfn __probe() {{}}"))
            .expect("parse synthetic gated fn");
        file.items
            .iter()
            .find_map(|it| match it {
                syn::Item::Fn(f) if f.sig.ident == "__probe" => Some(f.attrs.clone()),
                _ => None,
            })
            .expect("find __probe fn")
    }

    // PASS: `#[cfg(test)]`.
    assert!(
        carrier_for_test_gate_is_sanctioned(&attrs_from("#[cfg(test)]")),
        "self-test: `#[cfg(test)]` MUST be accepted"
    );
    // PASS: `#[cfg(any(test, feature = "test-support"))]`.
    assert!(
        carrier_for_test_gate_is_sanctioned(&attrs_from(
            "#[cfg(any(test, feature = \"test-support\"))]"
        )),
        "self-test: `#[cfg(any(test, feature = \"test-support\"))]` MUST be accepted"
    );
    // PASS: order-insensitive arm order.
    assert!(
        carrier_for_test_gate_is_sanctioned(&attrs_from(
            "#[cfg(any(feature = \"test-support\", test))]"
        )),
        "self-test: arm order MUST NOT matter — `any(feature = \"test-support\", test)` MUST be \
         accepted (token-tree parsed, not substring-ordered)"
    );

    // FIRE (RED): `#[cfg(any(test, feature = "test-support", unix))]` — the
    // `unix` arm makes it PRODUCTION-VISIBLE on Unix. This is the exact hole
    // the OLD substring matcher (`.contains("cfg (any (test") &&
    // .contains("feature = \"test-support\"")`) wrongly PASSED.
    assert!(
        !carrier_for_test_gate_is_sanctioned(&attrs_from(
            "#[cfg(any(test, feature = \"test-support\", unix))]"
        )),
        "self-test: `#[cfg(any(test, feature = \"test-support\", unix))]` MUST be REJECTED — the \
         `unix` arm makes the accessor production-visible on Unix builds"
    );
    // FIRE (RED): `#[cfg(any(test, feature = "test-support", feature =
    // "prod"))]` — the `feature = "prod"` arm is production-satisfiable. The
    // OLD substring matcher wrongly PASSED this too.
    assert!(
        !carrier_for_test_gate_is_sanctioned(&attrs_from(
            "#[cfg(any(test, feature = \"test-support\", feature = \"prod\"))]"
        )),
        "self-test: a third `feature = \"prod\"` arm MUST be REJECTED — it is production-satisfiable"
    );
    // FIRE (RED): `#[cfg(any(test, debug_assertions))]` — the debug-build hole.
    assert!(
        !carrier_for_test_gate_is_sanctioned(&attrs_from("#[cfg(any(test, debug_assertions))]")),
        "self-test: `#[cfg(any(test, debug_assertions))]` MUST be REJECTED — it is the debug-build \
         laundering hole the review found"
    );
    // FIRE (RED): ungated (no cfg at all).
    assert!(
        !carrier_for_test_gate_is_sanctioned(&[]),
        "self-test: an UNGATED carrier accessor MUST be rejected"
    );
    // FIRE (RED): an arbitrary production feature.
    assert!(
        !carrier_for_test_gate_is_sanctioned(&attrs_from("#[cfg(feature = \"oracle-gen\")]")),
        "self-test: a non-test-support `feature` gate MUST be rejected"
    );
    // FIRE (RED): TWO cfg attributes (compound gating) — the carrier gate must
    // be a SINGLE canonical cfg, not a stacked pair the substring scan ignored.
    assert!(
        !carrier_for_test_gate_is_sanctioned(&attrs_from("#[cfg(test)]\n        #[cfg(unix)]")),
        "self-test: a SECOND `#[cfg(unix)]` attribute MUST be REJECTED — stacked cfgs AND the \
         `unix` arm widen production reach"
    );
}

// ===========================================================================
// (5) [P1-C / P2] raise-seam returns a SEALED carrier, never a bare TypeExpr —
// and NO other public/restricted raise.rs fn returns a bare `TypeExpr`.
//
// The raise-side output seam `output_shell_raise_sealed` is `pub(super)`, so a
// `project_semantic_dispatch` SIBLING can reach it — but it returns a SEALED
// `Option<OutputTypeExpr>` the sibling cannot unwrap without a capability
// (proven by the DEBUG planted-exploit: the sibling call compiles, but
// `carrier.into_type_expr()` is E0061 — the capability arg is mandatory). The
// COMPILER cannot express "no `pub`/`pub(…)` fn in this module returns a bare
// `TypeExpr` raised from a node except the sanctioned
// `legacy_semantic_type_expr_bridge`". This guard pins that COMPREHENSIVELY:
//   (a) the removed bare delegators `output_shell_raise` /
//       `output_reduce_then_raise` do NOT exist;
//   (b) `output_shell_raise_sealed` returns the SEALED `Option<OutputTypeExpr>`;
//   (c) EVERY non-test public/restricted fn (inherent method OR free fn) whose
//       RETURN type mentions `TypeExpr` is EXACTLY the sanctioned interim
//       `legacy_semantic_type_expr_bridge` (the Kind-B bridge, `Option<TypeExpr>`)
//       — any OTHER `pub`/`pub(…)` `-> …TypeExpr…` fn is a re-opened bare-raise
//       laundering seam. (The still-private shell primitives
//       `raise_node_to_type_expr{,_inner}` + the private free fn
//       `semantic_primitive_to_type_expr` are module-private, so they are NOT
//       public/restricted and correctly excluded.)
// The scan is ALIAS-AWARE: a raise.rs `type X = …TypeExpr…` alias would let a
// restricted fn return `&X` instead of `&TypeExpr`; such an alias is collected
// and a return mentioning it is treated as TypeExpr-bearing. This is a
// guard-COMPLETENESS fix only — raise.rs production code is unchanged.
// ===========================================================================

const RAISE_REL: &str = "src/project_semantic_dispatch/raise.rs";

/// The sanctioned interim Kind-B bridge — the ONE non-test public/restricted
/// raise.rs fn allowed to return a bare `TypeExpr`.
const SANCTIONED_BARE_TYPE_EXPR_FN: &str = "legacy_semantic_type_expr_bridge";

/// One public/restricted raise.rs fn: `(name, is_test_gated, return_token_str)`.
/// Collected for both inherent methods and free fns; private (no-visibility)
/// fns are EXCLUDED at collection (the still-private shell primitives are
/// sanctioned and must not be flagged).
struct RaiseFnSig {
    name: String,
    is_test_gated: bool,
    /// The return type's `proc_macro2` token-string (` :: `-spaced), or empty
    /// for a unit return.
    ret: String,
    /// The return type's token stream (for the alias-aware mention check).
    ret_tokens: proc_macro2::TokenStream,
}

/// Does an item carry a `#[cfg(test)]` / `#[cfg(any(test, feature =
/// "test-support"))]` gate (so it is production-unreachable)?
fn attrs_are_test_gated(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|a| {
        if !a.path().is_ident("cfg") {
            return false;
        }
        matches!(&a.meta, syn::Meta::List(list)
            if cfg_is_exactly_test_or_test_support(list.tokens.clone()))
    })
}

/// Is a visibility `pub` or `pub(…)` (restricted)? A bare (inherited) visibility
/// is module-private and NOT in scope for the bare-TypeExpr-return ban.
fn vis_is_public_or_restricted(vis: &syn::Visibility) -> bool {
    matches!(
        vis,
        syn::Visibility::Public(_) | syn::Visibility::Restricted(_)
    )
}

/// Collect every public/restricted fn (inherent method in an inherent `impl`,
/// OR a free `fn`) in a raise-like `syn::File`, RECURSING into non-`#[cfg(test)]`
/// inline modules. Trait-impl methods are skipped (they implement an
/// external/owned trait contract, not a new raise seam); a `#[cfg(test)]` module
/// is skipped entirely (its fns are production-unreachable). Recursing modules
/// closes the gap where a future `mod inner { pub(crate) fn leak() -> TypeExpr }`
/// would have hidden a bare-`TypeExpr` raise seam from a file-scope-only scan.
fn raise_public_fn_sigs(file: &syn::File) -> Vec<RaiseFnSig> {
    fn ret_of(output: &syn::ReturnType) -> (String, proc_macro2::TokenStream) {
        match output {
            syn::ReturnType::Type(_, ty) => {
                (ty.to_token_stream().to_string(), ty.to_token_stream())
            }
            syn::ReturnType::Default => (String::new(), proc_macro2::TokenStream::new()),
        }
    }
    fn walk(items: &[syn::Item], out: &mut Vec<RaiseFnSig>) {
        for item in items {
            match item {
                syn::Item::Impl(imp) if imp.trait_.is_none() => {
                    for ii in &imp.items {
                        if let syn::ImplItem::Fn(f) = ii {
                            if !vis_is_public_or_restricted(&f.vis) {
                                continue;
                            }
                            let (ret, ret_tokens) = ret_of(&f.sig.output);
                            out.push(RaiseFnSig {
                                name: f.sig.ident.to_string(),
                                is_test_gated: attrs_are_test_gated(&f.attrs),
                                ret,
                                ret_tokens,
                            });
                        }
                    }
                }
                syn::Item::Fn(f) => {
                    if !vis_is_public_or_restricted(&f.vis) {
                        continue;
                    }
                    let (ret, ret_tokens) = ret_of(&f.sig.output);
                    out.push(RaiseFnSig {
                        name: f.sig.ident.to_string(),
                        is_test_gated: attrs_are_test_gated(&f.attrs),
                        ret,
                        ret_tokens,
                    });
                }
                // Recurse non-test inline modules; a `#[cfg(test)]` module is
                // production-unreachable and skipped wholesale.
                syn::Item::Mod(m) if !mod_is_cfg_test(&m.attrs) => {
                    if let Some((_, inner)) = &m.content {
                        walk(inner, out);
                    }
                }
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    walk(&file.items, &mut out);
    out
}

/// Collect the names of every `type X = …` alias (file scope, inside any module,
/// OR an `ImplItem::Type`) whose RHS is TRANSITIVELY `TypeExpr`-bearing — the
/// alias-launder vector (a restricted fn returns `&X` instead of `&TypeExpr`).
///
/// TRANSITIVE closure: a root `type A = …TypeExpr…` is flagged directly; a
/// `type B = …A…` whose RHS mentions an already-flagged alias is then ALSO
/// flagged, fixpoint-iterated until stable. This closes the chain-alias gap
/// (`type B = A; type A = TypeExpr` — a fn returning `&B` is now caught).
fn raise_type_expr_alias_names(file: &syn::File) -> Vec<String> {
    // (name, RHS token stream) for every type alias in the file.
    fn collect(items: &[syn::Item], out: &mut Vec<(String, proc_macro2::TokenStream)>) {
        for item in items {
            match item {
                syn::Item::Type(t) => out.push((t.ident.to_string(), t.ty.to_token_stream())),
                syn::Item::Mod(syn::ItemMod {
                    content: Some((_, inner)),
                    ..
                }) => collect(inner, out),
                syn::Item::Impl(imp) => {
                    for ii in &imp.items {
                        if let syn::ImplItem::Type(t) = ii {
                            out.push((t.ident.to_string(), t.ty.to_token_stream()));
                        }
                    }
                }
                _ => {}
            }
        }
    }
    fn tokens_mention_ident(ts: &proc_macro2::TokenStream, name: &str) -> bool {
        ts.clone().into_iter().any(|tt| match tt {
            proc_macro2::TokenTree::Ident(id) => id == name,
            proc_macro2::TokenTree::Group(g) => tokens_mention_ident(&g.stream(), name),
            _ => false,
        })
    }
    let mut aliases = Vec::new();
    collect(&file.items, &mut aliases);

    // Seed: aliases whose RHS mentions `TypeExpr` directly.
    let mut flagged: std::collections::BTreeSet<String> = aliases
        .iter()
        .filter(|(_, rhs)| token_stream_mentions_type_expr(rhs.clone()))
        .map(|(n, _)| n.clone())
        .collect();
    // Fixpoint: an alias whose RHS mentions any already-flagged alias is flagged.
    loop {
        let mut added = false;
        for (name, rhs) in &aliases {
            if flagged.contains(name) {
                continue;
            }
            if flagged.iter().any(|f| tokens_mention_ident(rhs, f)) {
                flagged.insert(name.clone());
                added = true;
            }
        }
        if !added {
            break;
        }
    }
    flagged.into_iter().collect()
}

/// Does a return token stream mention `TypeExpr` directly OR any of the
/// raise.rs-local `type … = …TypeExpr…` alias names (the alias-aware check)?
fn raise_return_is_type_expr_bearing(
    ret_tokens: &proc_macro2::TokenStream,
    alias_names: &[String],
) -> bool {
    if token_stream_mentions_type_expr(ret_tokens.clone()) {
        return true;
    }
    // Alias-aware: a return mentioning a TypeExpr-aliasing local alias name.
    let mention = |name: &str| {
        ret_tokens.clone().into_iter().any(|tt| {
            fn scan(ts: proc_macro2::TokenStream, name: &str) -> bool {
                ts.into_iter().any(|tt| match tt {
                    proc_macro2::TokenTree::Ident(id) => id == name,
                    proc_macro2::TokenTree::Group(g) => scan(g.stream(), name),
                    _ => false,
                })
            }
            match tt {
                proc_macro2::TokenTree::Ident(id) => id == name,
                proc_macro2::TokenTree::Group(g) => scan(g.stream(), name),
                _ => false,
            }
        })
    };
    alias_names.iter().any(|n| mention(n))
}

/// The comprehensive raise.rs bare-TypeExpr-return violation set, factored out
/// so the self-test can feed it a synthetic raise-like file. Empty ⇒ the seam
/// is sealed: no public/restricted raise fn returns a bare `TypeExpr` except
/// the sanctioned bridge, and `output_shell_raise_sealed` returns the carrier.
fn raise_bare_type_expr_violations(file: &syn::File) -> Vec<String> {
    let sigs = raise_public_fn_sigs(file);
    let alias_names = raise_type_expr_alias_names(file);
    let mut violations = Vec::new();

    // (a) Removed bare delegators must NOT exist.
    for removed in ["output_shell_raise", "output_reduce_then_raise"] {
        if sigs.iter().any(|s| s.name == removed) {
            violations.push(format!(
                "the bare raise delegator `{removed}` must NOT exist — it handed a \
                 `project_semantic_dispatch` sibling a bare `TypeExpr` with no capability. The \
                 output seam is `output_shell_raise_sealed` (SEALED `Option<OutputTypeExpr>`)"
            ));
        }
    }

    // (c) Every non-test public/restricted fn returning a TypeExpr-bearing type
    // must be EXACTLY the sanctioned bridge.
    for s in &sigs {
        if s.is_test_gated {
            continue; // test harnesses (materialize_type_expr, *_for_test) excluded
        }
        if !raise_return_is_type_expr_bearing(&s.ret_tokens, &alias_names) {
            continue;
        }
        if s.name == SANCTIONED_BARE_TYPE_EXPR_FN {
            continue; // the sanctioned interim Kind-B bridge
        }
        violations.push(format!(
            "`{}` is a non-test public/restricted raise.rs fn returning a `TypeExpr`-bearing type \
             (`{}`) — only the sanctioned interim Kind-B bridge `{SANCTIONED_BARE_TYPE_EXPR_FN}` \
             may return a bare `TypeExpr`; route any new output sink through the sealed \
             `OutputProjector` capability (a sealed `OutputTypeExpr` / `MaterializedOutputTypeExpr` \
             carrier), never a bare reverse-raise seam",
            s.name,
            normalize_mod_path(&s.ret)
        ));
    }

    violations
}

#[test]
fn raise_output_seam_returns_sealed_carrier_not_bare_type_expr() {
    let src = read_rel(RAISE_REL);
    let file = syn::parse_file(&src).expect("parse raise.rs");

    // (a) + (c) comprehensive bare-TypeExpr-return ban.
    let violations = raise_bare_type_expr_violations(&file);
    assert!(
        violations.is_empty(),
        "STRUCTURAL FENCE [P1-C / P2]: a raise.rs boundary returns a bare `TypeExpr` — the \
         reverse-raise output seam must hand back a SEALED carrier (or be the sanctioned interim \
         Kind-B bridge):\n{}",
        violations.join("\n")
    );

    // (b) The sealed shell seam must EXIST and return the sealed carrier (NOT a
    // bare `Option<TypeExpr>`) — anti-vacuity: if the seam vanished, the fence
    // is gone.
    let sigs = raise_public_fn_sigs(&file);
    let sealed = sigs
        .iter()
        .find(|s| s.name == "output_shell_raise_sealed")
        .unwrap_or_else(|| {
            panic!(
                "STRUCTURAL FENCE [P1-C]: the sealed shell seam `output_shell_raise_sealed` must \
                 exist in {RAISE_REL}"
            )
        });
    let ret = normalize_mod_path(&sealed.ret);
    assert!(
        ret.contains("OutputTypeExpr"),
        "STRUCTURAL FENCE [P1-C]: `output_shell_raise_sealed` must return a SEALED `OutputTypeExpr` \
         carrier (`Option<OutputTypeExpr>`), never a bare `Option<TypeExpr>`. Its return type is \
         `{}`",
        sealed.ret
    );
}

#[test]
fn raise_output_seam_self_test_discriminates() {
    fn violations(src: &str) -> Vec<String> {
        let file = syn::parse_file(src).expect("parse synthetic raise-like file");
        raise_bare_type_expr_violations(&file)
    }

    // FIRE (RED): a NEW `pub(crate) fn sneaky(&self) -> Option<TypeExpr>` is a
    // re-opened bare-raise seam — the exact [P2] residual.
    let sneaky = r#"
        impl D {
            pub(crate) fn sneaky(&self, node: SemanticNodeId) -> Option<TypeExpr> {
                self.raise_node_to_type_expr(node)
            }
        }
    "#;
    assert!(
        violations(sneaky).iter().any(|m| m.contains("`sneaky`")),
        "self-test: a NEW `pub(crate) fn sneaky(&self) -> Option<TypeExpr>` MUST fire — only the \
         sanctioned bridge may return a bare `TypeExpr`; got {:?}",
        violations(sneaky)
    );

    // FIRE (RED): the ALIAS launder — `type Body = TypeExpr; pub(crate) fn
    // alias_seam(&self) -> Option<Body>` — fires via the alias-aware check.
    let alias_seam = r#"
        type Body = TypeExpr;
        impl D {
            pub(crate) fn alias_seam(&self, node: SemanticNodeId) -> Option<Body> {
                self.raise_node_to_type_expr(node)
            }
        }
    "#;
    assert!(
        violations(alias_seam)
            .iter()
            .any(|m| m.contains("`alias_seam`")),
        "self-test: a `pub(crate) fn alias_seam(&self) -> Option<Body>` where `type Body = \
         TypeExpr` MUST fire (alias-aware); got {:?}",
        violations(alias_seam)
    );

    // FIRE (RED): a re-added bare delegator `output_shell_raise` fires.
    let readded = r#"
        impl D {
            pub(super) fn output_shell_raise(&self, node: SemanticNodeId) -> Option<TypeExpr> {
                self.raise_node_to_type_expr(node)
            }
        }
    "#;
    assert!(
        violations(readded)
            .iter()
            .any(|m| m.contains("output_shell_raise")),
        "self-test: a re-added bare delegator `output_shell_raise` MUST fire; got {:?}",
        violations(readded)
    );

    // FIRE (RED): a bare-`TypeExpr` raise seam nested in a NON-TEST inline module
    // fires (the module-recursion gap — a file-scope-only scan would have missed
    // `mod inner { pub(crate) fn nested_leak() -> TypeExpr }`).
    let nested = r#"
        mod inner {
            pub(crate) fn nested_leak(node: SemanticNodeId) -> TypeExpr {
                TypeExpr::Unknown { raw: String::new() }
            }
        }
    "#;
    assert!(
        violations(nested).iter().any(|m| m.contains("nested_leak")),
        "self-test: a bare-`TypeExpr` raise seam in a NON-TEST inline module MUST fire (module \
         recursion); got {:?}",
        violations(nested)
    );

    // PASS: a bare-`TypeExpr` seam inside a `#[cfg(test)]` MODULE is
    // production-unreachable and excluded (the module recursion skips cfg(test)
    // modules wholesale).
    let test_mod = r#"
        #[cfg(test)]
        mod t {
            pub(crate) fn test_only_leak(node: SemanticNodeId) -> TypeExpr {
                TypeExpr::Unknown { raw: String::new() }
            }
        }
    "#;
    assert!(
        violations(test_mod).is_empty(),
        "self-test: a bare-`TypeExpr` seam inside a `#[cfg(test)]` module is production-unreachable \
         and MUST NOT fire; got {:?}",
        violations(test_mod)
    );

    // FIRE (RED): a TRANSITIVE alias chain — `type Outer = Inner; type Inner =
    // TypeExpr; pub(crate) fn chain_seam() -> Option<Outer>` — fires via the
    // transitive-closure alias resolution (a single-level alias-name check would
    // have missed `Outer`).
    let transitive = r#"
        type Outer = Inner;
        type Inner = TypeExpr;
        impl D {
            pub(crate) fn chain_seam(&self, node: SemanticNodeId) -> Option<Outer> {
                self.raise_node_to_type_expr(node)
            }
        }
    "#;
    assert!(
        violations(transitive)
            .iter()
            .any(|m| m.contains("chain_seam")),
        "self-test: a `pub(crate) fn chain_seam() -> Option<Outer>` where `type Outer = Inner` and \
         `type Inner = TypeExpr` MUST fire (transitive alias closure); got {:?}",
        violations(transitive)
    );

    // PASS: the sanctioned bridge + the sealed seam + a PRIVATE shell primitive
    // produce ZERO violations. (`raise_node_to_type_expr` has no visibility
    // modifier → module-private → excluded; the bridge is the one allowed bare
    // return; the sealed seam returns the carrier; the test harness is
    // `#[cfg(test)]`-gated → excluded.)
    let good = r#"
        impl D {
            fn raise_node_to_type_expr(&self, node: SemanticNodeId) -> Option<TypeExpr> { None }
            pub(super) fn output_shell_raise_sealed(
                &self, node: SemanticNodeId,
            ) -> Option<super::output_materialization::OutputTypeExpr> {
                self.raise_node_to_type_expr(node).map(OutputTypeExpr::from_raise)
            }
            pub(crate) fn legacy_semantic_type_expr_bridge(
                &self, node: SemanticNodeId,
            ) -> Option<TypeExpr> {
                self.raise_node_to_type_expr(node)
            }
            #[cfg(test)]
            pub(crate) fn materialize_type_expr(&self, handle: HotTypeRef) -> TypeExpr {
                TypeExpr::Unknown { raw: String::new() }
            }
        }
        fn semantic_primitive_to_type_expr(kind: SemanticPrimitiveKind) -> TypeExpr {
            TypeExpr::Primitive(kind)
        }
    "#;
    assert!(
        violations(good).is_empty(),
        "self-test: the sanctioned bridge + sealed seam + private shell primitive + private free \
         fn + test-gated harness MUST pass with ZERO violations; got {:?}",
        violations(good)
    );

    // The sealed-return classifier still discriminates the seam's own return.
    let is_sealed = |ret: &str| normalize_mod_path(ret).contains("OutputTypeExpr");
    assert!(
        is_sealed("Option < super :: output_materialization :: OutputTypeExpr >"),
        "self-test: a sealed `Option<OutputTypeExpr>` return MUST classify as sealed"
    );
    assert!(
        !is_sealed("Option < TypeExpr >"),
        "self-test: a bare `Option<TypeExpr>` return MUST classify as NOT sealed"
    );
}

// ===========================================================================
// (6) Carrier self-type classification recognises REFERENCE self-types.
//
// `impl_self_ty_last_ident` (the shared carrier-name classifier the owner-file
// guards in (2) depend on) unwraps `Type::Reference` / `Type::Group` /
// `Type::Paren` to the inner path, so an escaping carrier impl written on a
// REFERENCE self-type (`impl AsRef<TypeExpr> for &OutputTypeExpr`,
// `impl Deref for &MaterializedOutputTypeExpr`) is classified by carrier name
// rather than SKIPPED. These self-tests pin that classifier behaviour so the
// owner-file inherent-method / payload-field guards cannot be evaded by writing
// the escape on a reference self-type.
// ===========================================================================

#[test]
fn fence_shape_classifies_reference_self_types() {
    // Reference self-type → unwrapped to the carrier ident.
    let ref_ty: syn::Type = syn::parse_str("&OutputTypeExpr").expect("parse &OutputTypeExpr");
    assert_eq!(
        impl_self_ty_last_ident(&ref_ty).as_deref(),
        Some("OutputTypeExpr"),
        "carrier classifier: `&OutputTypeExpr` MUST classify as `OutputTypeExpr` (reference self-type unwrap)"
    );
    let ref_mut_ty: syn::Type =
        syn::parse_str("&mut MaterializedOutputTypeExpr").expect("parse &mut carrier");
    assert_eq!(
        impl_self_ty_last_ident(&ref_mut_ty).as_deref(),
        Some("MaterializedOutputTypeExpr"),
        "carrier classifier: `&mut MaterializedOutputTypeExpr` MUST classify as `MaterializedOutputTypeExpr`"
    );
    // Parenthesised self-type → unwrapped.
    let paren_ty: syn::Type = syn::parse_str("(OutputTypeExpr)").expect("parse (carrier)");
    assert_eq!(
        impl_self_ty_last_ident(&paren_ty).as_deref(),
        Some("OutputTypeExpr"),
        "carrier classifier: `(OutputTypeExpr)` MUST classify as `OutputTypeExpr` (paren self-type unwrap)"
    );
    // Plain path still works.
    let path_ty: syn::Type =
        syn::parse_str("crate::x::OutputTypeExpr").expect("parse path carrier");
    assert_eq!(
        impl_self_ty_last_ident(&path_ty).as_deref(),
        Some("OutputTypeExpr"),
        "carrier classifier: a plain path self-type MUST still classify by last segment"
    );
}

#[test]
fn fence_shape_inventory_catches_reference_self_type_escapes() {
    // The carrier-trait-violation detector (mirroring the inventory's logic)
    // must now FIRE on an escaping impl written on a REFERENCE self-type — the
    // exact escape the old `Type::Path`-only classifier SKIPPED.
    fn carrier_trait_violations(src: &str) -> Vec<String> {
        let file = syn::parse_file(src).expect("parse synthetic");
        const CARRIERS: &[&str] = &["OutputTypeExpr", "MaterializedOutputTypeExpr"];
        let mut v = Vec::new();
        for item in &file.items {
            if let syn::Item::Impl(imp) = item {
                let Some(self_name) = impl_self_ty_last_ident(&imp.self_ty) else {
                    continue;
                };
                if !CARRIERS.contains(&self_name.as_str()) {
                    continue;
                }
                if let Some((_, trait_path, _)) = &imp.trait_ {
                    if let Some(seg) = trait_path.segments.last() {
                        let tn = seg.ident.to_string();
                        if tn == "Deref" || tn == "DerefMut" || tn == "AsRef" || tn == "Borrow" {
                            v.push(format!("impl {tn} for {self_name}"));
                        }
                    }
                }
            }
        }
        v
    }

    // FIRE: `impl AsRef<TypeExpr> for &OutputTypeExpr` (reference self-type).
    let ref_asref = r#"
        impl AsRef<TypeExpr> for &OutputTypeExpr {
            fn as_ref(&self) -> &TypeExpr { &self.0 }
        }
    "#;
    assert!(
        !carrier_trait_violations(ref_asref).is_empty(),
        "carrier classifier: `impl AsRef<TypeExpr> for &OutputTypeExpr` (REFERENCE self-type) MUST be caught — it \
         is a real `TypeExpr` escape the old `Type::Path`-only classifier skipped"
    );
    // FIRE: `impl Deref for &MaterializedOutputTypeExpr`.
    let ref_deref = r#"
        impl std::ops::Deref for &MaterializedOutputTypeExpr {
            type Target = TypeExpr;
            fn deref(&self) -> &TypeExpr { &self.type_expr.0 }
        }
    "#;
    assert!(
        !carrier_trait_violations(ref_deref).is_empty(),
        "carrier classifier: `impl Deref for &MaterializedOutputTypeExpr` (REFERENCE self-type) MUST be caught"
    );
    // PASS: an unrelated `impl Clone for &OutputTypeExpr` does NOT fire.
    let ref_clone = r#"
        impl Clone for &OutputTypeExpr {
            fn clone(&self) -> Self { *self }
        }
    "#;
    assert!(
        carrier_trait_violations(ref_clone).is_empty(),
        "carrier classifier: an unrelated `impl Clone for &OutputTypeExpr` MUST NOT fire"
    );
}

// ===========================================================================
// (7) TestOutputCap is not visible or mintable in non-test builds.
//
// `TestOutputCap` is a MINTABLE `OutputProjector` capability — its `new` lets
// any holder obtain the capability and unwrap a sealed carrier. It exists ONLY
// so the carrier round-trip / reduce / projector-peek test suites can drive the
// boundary methods without holding a real sink's capability. Its STRUCT, its
// `new` inherent impl, and its `Sealed` / `OutputProjector` impls are ALL
// `#[cfg(test)]`-gated, so a non-test build (plain `cargo build`, release) does
// NOT compile them — the strongest possible production-absence guarantee.
//
// The production-surface invariant ("`TestOutputCap` is absent / not mintable
// in non-test builds") is therefore COMPILER-ENFORCED. The residual the
// compiler cannot express is a future WIDENING of that gate — e.g. from
// `#[cfg(test)]` to `#[cfg(any(test, debug_assertions))]` — which would make
// the mintable capability DEBUG-REACHABLE (the same carrier-unwrap hole the
// `_for_test` accessor gate in (4) prevents, `debug_assertions` being ON in
// ordinary debug builds). This guard pins that every `TestOutputCap` capability
// item is gated EXACTLY `#[cfg(test)]` or
// `#[cfg(any(test, feature = "test-support"))]` (the production-unreachable
// gates), reusing the shared EXACT recogniser `cfg_is_exactly_test_or_test_support`.
//
// It walks recursively into the inline modules: `TestOutputCap` lives inside
// `mod projector` after the payload-vault restructure. The module-topology
// guard in (2) pins the EXACT inline-module set (projector / projector::sealed /
// carrier / carrier::payload) and BANS item-macro / include / attribute-macro
// injection, so an unsanctioned `mod shadow { TestOutputCap }` or a hidden
// macro-injected module is rejected THERE — the recursion here is bounded to the
// sanctioned vault scopes. This is a by-NAME identity scanner of the
// `TestOutputCap` capability items, hence the guard-local Structural-Confinement
// record:
//
// ```text
// scanner_invariant: test_output_cap_capability_items_are_exactly_test_gated
// scanner_justification: Rust cannot express "the mintable TestOutputCap capability's struct + new + Sealed/OutputProjector impls must stay test-entailing-gated"; a widened cfg(any(test, debug_assertions)) would compile a debug-reachable mintable capability.
// mechanism_ruling: structural-confinement-first — the #[cfg(test)] compiler-gating is the primary production-absence guarantee and the (2) module-topology guard bounds the inline-mod set (so the nesting vector is pinned, not unbounded); this by-name entailment check is the bounded residual for a future gate WIDENING the compiler cannot reject.
// hardening_rounds: 0
// hardening_history: initial entailment check; walks the sanctioned inline vault modules (TestOutputCap relocated into mod projector by the payload-vault restructure).
// ```
// ===========================================================================

/// One `TestOutputCap` capability item kind, for per-kind messaging.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TestCapKind {
    Struct,
    NewImpl,
    SealedImpl,
    ProjectorImpl,
}

impl TestCapKind {
    fn describe(self) -> &'static str {
        match self {
            TestCapKind::Struct => "the `TestOutputCap` STRUCT",
            TestCapKind::NewImpl => "the `TestOutputCap::new()` inherent impl block",
            TestCapKind::SealedImpl => "the `impl Sealed for TestOutputCap`",
            TestCapKind::ProjectorImpl => "the `impl OutputProjector for TestOutputCap`",
        }
    }
}

/// Whether an item's attribute set is EXACTLY a single production-unreachable
/// test gate (`#[cfg(test)]` or `#[cfg(any(test, feature = "test-support"))]`),
/// reusing the shared EXACT recogniser. A bare-`#[cfg]`-less item, a
/// `debug_assertions`-bearing gate, or a stacked second cfg is NOT sanctioned.
fn item_is_exactly_test_gated(attrs: &[syn::Attribute]) -> bool {
    let cfgs: Vec<&syn::Attribute> = attrs.iter().filter(|a| a.path().is_ident("cfg")).collect();
    if cfgs.len() != 1 {
        return false;
    }
    let syn::Meta::List(list) = &cfgs[0].meta else {
        return false;
    };
    cfg_is_exactly_test_or_test_support(list.tokens.clone())
}

/// Collect every `TestOutputCap` capability item in `file` as
/// `(kind, is_exactly_test_gated)`. Walks recursively into inline modules —
/// `TestOutputCap` (its struct, `new` impl, `Sealed` impl, `OutputProjector`
/// impl) lives inside `mod projector` after the payload-vault restructure. The
/// module-topology guard in (2) pins the EXACT inline-module set, so the only
/// inline scopes this walk can encounter are the sanctioned vault modules; the
/// gate INVARIANT (every TestOutputCap item EXACTLY test-gated, exactly one of
/// each kind) is unchanged — only the walk descends.
fn test_cap_observations(file: &syn::File) -> Vec<(TestCapKind, bool)> {
    fn walk(items: &[syn::Item], obs: &mut Vec<(TestCapKind, bool)>) {
        for item in items {
            match item {
                syn::Item::Struct(s) if s.ident == "TestOutputCap" => {
                    obs.push((TestCapKind::Struct, item_is_exactly_test_gated(&s.attrs)));
                }
                syn::Item::Impl(imp) => {
                    let Some(self_name) = impl_self_ty_last_ident(&imp.self_ty) else {
                        continue;
                    };
                    if self_name != "TestOutputCap" {
                        continue;
                    }
                    let gated = item_is_exactly_test_gated(&imp.attrs);
                    match &imp.trait_ {
                        None => {
                            let has_new = imp.items.iter().any(
                                |ii| matches!(ii, syn::ImplItem::Fn(f) if f.sig.ident == "new"),
                            );
                            if has_new {
                                obs.push((TestCapKind::NewImpl, gated));
                            }
                        }
                        Some((_, trait_path, _)) => {
                            match trait_path.segments.last().map(|s| s.ident.to_string()) {
                                Some(t) if t == "Sealed" => {
                                    obs.push((TestCapKind::SealedImpl, gated))
                                }
                                Some(t) if t == "OutputProjector" => {
                                    obs.push((TestCapKind::ProjectorImpl, gated))
                                }
                                _ => {}
                            }
                        }
                    }
                }
                syn::Item::Mod(syn::ItemMod {
                    content: Some((_, inner)),
                    ..
                }) => walk(inner, obs),
                _ => {}
            }
        }
    }
    let mut obs = Vec::new();
    walk(&file.items, &mut obs);
    obs
}

/// One violation string per offending `TestOutputCap` item; empty ⇒ the gate
/// holds. EACH observed item of EACH kind must be EXACTLY test-gated, AND there
/// must be EXACTLY ONE definition per kind (a duplicate widens the mintable
/// surface; a missing one means the gate is gone).
fn test_cap_gate_violations(file: &syn::File) -> Vec<String> {
    let obs = test_cap_observations(file);
    let mut violations: Vec<String> = Vec::new();
    for (kind, gated) in &obs {
        if !*gated {
            violations.push(format!(
                "{} is NOT gated EXACTLY `#[cfg(test)]` / `#[cfg(any(test, feature = \
                 \"test-support\"))]` — a widened gate (e.g. `#[cfg(any(test, debug_assertions))]`) \
                 would compile a DEBUG-REACHABLE mintable `OutputProjector` capability (the cap and \
                 its `new` are `pub(crate)`), the carrier-unwrap hole this gate exists to prevent",
                kind.describe()
            ));
        }
    }
    for kind in [
        TestCapKind::Struct,
        TestCapKind::NewImpl,
        TestCapKind::SealedImpl,
        TestCapKind::ProjectorImpl,
    ] {
        let count = obs.iter().filter(|(k, _)| *k == kind).count();
        if count != 1 {
            violations.push(format!(
                "expected EXACTLY ONE definition of {}, found {count} — the sanctioned shape is a \
                 single test-gated definition of each kind; a missing one leaves the cap un-gated \
                 and a duplicate widens the mintable-capability surface",
                kind.describe()
            ));
        }
    }
    violations
}

#[test]
fn test_output_cap_not_visible_or_mintable_in_non_test_builds() {
    let src = read_rel(OWNER_REL);
    let file = syn::parse_file(&src).expect("parse output_materialization.rs");
    let violations = test_cap_gate_violations(&file);
    assert!(
        violations.is_empty(),
        "`TestOutputCap` non-test-build gate violation(s) — a debug-reachable or duplicate mintable \
         `OutputProjector` capability is a carrier-unwrap hole the compiler privacy does NOT \
         catch:\n{}",
        violations.join("\n")
    );
}

#[test]
fn test_output_cap_gate_self_test_discriminates() {
    fn gate(src: &str) -> Vec<String> {
        let file = syn::parse_file(src).expect("parse synthetic TestOutputCap source");
        test_cap_gate_violations(&file)
    }

    // The four canonical, correctly-test-gated definitions (one of each kind) —
    // the PASS shape and the trailing "good" items in the widening/duplicate
    // cases.
    const CANONICAL_TEST_GATED: &str = r#"
        #[cfg(test)]
        pub(crate) struct TestOutputCap<'disp, 'ctx> {
            dispatch: &'disp ProjectSemanticDispatch<'ctx>,
        }
        #[cfg(test)]
        impl<'disp, 'ctx> TestOutputCap<'disp, 'ctx> {
            pub(crate) fn new(dispatch: &'disp ProjectSemanticDispatch<'ctx>) -> Self {
                Self { dispatch }
            }
        }
        #[cfg(test)]
        impl sealed::Sealed for TestOutputCap<'_, '_> {}
        #[cfg(test)]
        impl OutputProjector for TestOutputCap<'_, '_> {
            fn dispatch(&self) -> &ProjectSemanticDispatch<'_> { self.dispatch }
        }
    "#;

    // PASS: the correct single-test-gated shape produces ZERO violations.
    assert!(
        gate(CANONICAL_TEST_GATED).is_empty(),
        "self-test: the canonical single-test-gated `TestOutputCap` shape MUST pass; got: {:?}",
        gate(CANONICAL_TEST_GATED)
    );

    // RED: a single `#[cfg(any(test, debug_assertions))]`-widened struct is
    // debug-reachable (does NOT entail test alone) and fires the entailment
    // rule — the exact carrier-unwrap hole the gate guards.
    let widened = r#"
        #[cfg(any(test, debug_assertions))]
        pub(crate) struct TestOutputCap<'disp, 'ctx> {
            dispatch: &'disp ProjectSemanticDispatch<'ctx>,
        }
        #[cfg(test)]
        impl<'disp, 'ctx> TestOutputCap<'disp, 'ctx> {
            pub(crate) fn new(dispatch: &'disp ProjectSemanticDispatch<'ctx>) -> Self {
                Self { dispatch }
            }
        }
        #[cfg(test)]
        impl sealed::Sealed for TestOutputCap<'_, '_> {}
        #[cfg(test)]
        impl OutputProjector for TestOutputCap<'_, '_> {
            fn dispatch(&self) -> &ProjectSemanticDispatch<'_> { self.dispatch }
        }
    "#;
    assert!(
        gate(widened)
            .iter()
            .any(|v| v.contains("STRUCT") && v.contains("test-support")),
        "self-test: a `#[cfg(any(test, debug_assertions))]`-widened TestOutputCap STRUCT MUST fire \
         the entailment violation (debug-reachable); got: {:?}",
        gate(widened)
    );

    // RED (anti-vacuity): a SINGLE un-gated (`#[cfg]`-less) struct fires (an
    // un-gated item is production-reachable) — the gate is not vacuously
    // satisfied by a lone definition.
    let ungated = r#"
        pub(crate) struct TestOutputCap<'disp, 'ctx> {
            dispatch: &'disp ProjectSemanticDispatch<'ctx>,
        }
        #[cfg(test)]
        impl<'disp, 'ctx> TestOutputCap<'disp, 'ctx> {
            pub(crate) fn new(dispatch: &'disp ProjectSemanticDispatch<'ctx>) -> Self {
                Self { dispatch }
            }
        }
        #[cfg(test)]
        impl sealed::Sealed for TestOutputCap<'_, '_> {}
        #[cfg(test)]
        impl OutputProjector for TestOutputCap<'_, '_> {
            fn dispatch(&self) -> &ProjectSemanticDispatch<'_> { self.dispatch }
        }
    "#;
    assert!(
        gate(ungated)
            .iter()
            .any(|v| v.contains("STRUCT") && v.contains("test-support")),
        "self-test: a single un-gated `TestOutputCap` STRUCT MUST fire the entailment violation \
         (anti-vacuity); got: {:?}",
        gate(ungated)
    );

    // RED: a SECOND `#[cfg(test)]` struct (both test-gated, so the entailment
    // rule alone passes) fires on the per-kind COUNT rule — a sneaky duplicate
    // widens the mint surface even when test-gated.
    let duplicate = format!(
        r#"
        #[cfg(test)]
        pub(crate) struct TestOutputCap<'disp, 'ctx> {{
            dispatch: &'disp ProjectSemanticDispatch<'ctx>,
        }}
        {CANONICAL_TEST_GATED}
    "#
    );
    assert!(
        gate(&duplicate)
            .iter()
            .any(|v| v.contains("STRUCT") && v.contains("found 2")),
        "self-test: a SECOND `#[cfg(test)]` TestOutputCap STRUCT MUST fire the count violation \
         (both test-gated, so only the count rule catches it); got: {:?}",
        gate(&duplicate)
    );

    // RED (anti-vacuity, missing kind): the struct alone, with the impls absent,
    // fires the count rule for each missing kind (count 0 != 1).
    let only_struct = r#"
        #[cfg(test)]
        pub(crate) struct TestOutputCap<'disp, 'ctx> {
            dispatch: &'disp ProjectSemanticDispatch<'ctx>,
        }
    "#;
    let v = gate(only_struct);
    assert!(
        v.iter()
            .any(|m| m.contains("OutputProjector") && m.contains("found 0")),
        "self-test: a missing `impl OutputProjector for TestOutputCap` MUST fire the count rule \
         (anti-vacuity for a removed gate kind); got: {v:?}"
    );
}

// ===========================================================================
// (D1) STRUCTURAL cross-sink raw-authority → `TypeExpr` boundary guard.
//
// SCOPE — Kind-A / PUBLICATION boundaries. This guard closes the Kind-A
// PUBLICATION raw-authority → `TypeExpr` boundary (the true OUTPUT sinks: the
// per-macro projectors, the projector cache-key layer, the framework-surface
// resolution/normalize sink, and the query-engine surface projector), via the
// sealed admitted-token chain (the compiler primary) + this structural cross-
// sink scanner. It is NOT a claim that NO raw-node → `TypeExpr` boundary remains
// anywhere: the named Kind-B raise-then-decide semantic-decision residual (the
// `legacy_semantic_type_expr_bridge` route reached through `execute_to_type_expr`
// / `project_slot_binding_member_with_terminal_id`) is DEFERRED to the Kind-B
// graph-native conversion (docs/arch/parselower-design.md
// `Stage8-A4-kind-b-graph-native-conversion`) and pinned separately by
// `residual_output_materialization_bridge_no_new_kind_b_references` +
// `kind_b_raise_then_decide_entrypoints_pinned`. Those Kind-B sites are NOT
// publication sinks and are intentionally outside this guard's scan.
//
// Replaces the prior name-based single-file `output_sink` closed-allowlist
// boundary check. Across the registered output sinks above this guard
// classifies BOTH sides of every reachable production fn / method / trait-item
// TRANSITIVELY and FAILS any that pairs a `TypeExpr`-bearing OUTPUT (or a
// mutated DTO out-param) with a FORGEABLE RAW-AUTHORITY input — except a CLOSED
// ALLOWLIST of true sink-local APIs.
//
// ANTI-RECURRENCE PROPERTY (the whole reason this replaces the name-based
// guard): "TypeExpr-bearing" is decided by STRUCTURAL FIELD-CLOSURE from
// `TypeExpr` over the type's own field graph — NOT a hand-list of DTO names. A
// type is TypeExpr-bearing iff it IS `TypeExpr`, OR any of its fields / enum
// variants / element types (transitively, following struct fields, enum
// variant fields, `Vec<_>` / `Option<_>` / `Arc<_>` / `Box<_>` / tuple element
// types, and `type X = ...` aliases) reaches `TypeExpr`. The Output-seeds list
// below is the EXPECTED-flag oracle (used by the self-test + to bound which
// crates' type defs the closure reads), NOT a closed spelled-name allowlist a
// newly-named DTO could slip past: a new `struct FooSurface { members:
// Vec<ProjectedMember> }` is flagged by the closure WITHOUT anyone adding
// "FooSurface" to a list.
//
// THE GUARD IS A STRUCTURALLY-COMPLETE (vs the old name-based pin) RESIDUAL
// SUPPLEMENT behind the sealed-token compiler primary over the registered
// PUBLICATION sinks — the production COMPLETENESS guarantee is the sealed token
// (forging one is a compile error); this scanner is the residual cross-module
// pairing supplement, NOT a load-bearing replacement:
//   - MODULE-QUALIFIED `(module, name)` type identity: the closure graph is
//     keyed by `TypeDefId { module, name }`, NOT a bare final segment. Each
//     referenced type is resolved to a concrete `TypeDefId` by an 80/20,
//     FAIL-CLOSED identity classifier for the CURRENT production reference
//     shapes (`resolve_type_ref`) — NOT a complete Rust name resolver. It covers
//     the COMMON in-tree paths: own-module defs, rooted `crate` / `self` /
//     `super` direct paths (a relative `super` rebased onto the referencing
//     module, never escaping above the crate root), exact `pub` / `pub(crate)`
//     re-exports (TARGET module the candidate's real home EXACTLY, keyed by the
//     NORMALIZED absolute written path — e.g.
//     `crate::semantic_query::BudgetExceededFailure` through the `semantic_query`
//     re-export of the `shallow_file_state` def), ordinary file imports (a `use`
//     import, incl. `as Alias` renames, whose TARGET resolves by proof), and the
//     audited `registry_decl` private `use`-binding chain (the genuine
//     `super::ResolvedTypeDeclaration` chain through a parent module's private
//     `use`). A COLLIDING name (`crate::semantic_query::IndexSignature` —
//     SemanticNodeId fields, the authority seed — vs `verter_type_expr::
//     IndexSignature` — TypeExpr fields, an already-lowered-IR bearing leaf) is
//     disambiguated into DISTINCT ids; same-name re-export type-aliases collapse
//     onto their target. A reference the classifier cannot resolve to a single
//     target stays `Unresolved` and is caught FAIL-CLOSED at the boundary
//     completeness checks; the safe-input / construction-chain token sets are
//     `(module, name)`-keyed with a FAIL-CLOSED anti-vacuity rail (a
//     missing/moved token fires), and a bare-name collision is not "accepted
//     because one is bearing" — the distinct ids each stand on their own merits.
//
//     RESIDUAL forged shapes are OUTSIDE this classifier's proof claim — they
//     are adversarial relative to this crate's current production sink-token
//     references (each sanctioned token is uniquely named, so even an
//     over-resolution lands on the single genuine def), and the
//     compiler-enforced sealed-token boundary remains the production guarantee.
//     They are an ACCEPTED defense-in-depth debt (see
//     `docs/arch/output-projector-residual-guard-debt.md`), NOT a tightening
//     backlog. The disclosure is by ROOT-CAUSE CLASS (complete-by-construction —
//     any specific instance below is subsumed by its class), NOT an exhaustive
//     per-instance list:
//       (A) SYNTACTIC `use` collection. ALL THREE `use`-collectors
//       (`collect_use_index` for file imports, `collect_reexport_index` for the
//       `pub`/`pub(crate)` re-export proof rail, `collect_use_binding_index` for
//       the intra-crate use-binding chain) are SYNTACTIC: none evaluates
//       item-level `cfg` / `cfg_attr`, and `collect_use_index` additionally
//       ignores module nesting (it is file-wide). So a `cfg` / `cfg_attr`-gated
//       `use` (active cfg not evaluated) — and, for the file-import rail, a `use`
//       inside an INLINE module incl. a `#[cfg(test)] mod` — over-contributes a
//       binding/re-export/chain edge the active build may not have, across all
//       three rails. (The `mod_is_cfg_test` skip is the SINK-FN collector's
//       `visit_item_mod`, NOT any of these `use`-collectors.)
//       (B) NON-PROOF bare-name fallback. The unqualified arms resolve by
//       UNIQUENESS / FIRST-MATCH rather than proof whenever a unique single
//       PROVEN target is not found: the `candidates.len() == 1` global-uniqueness
//       fallback (arm (d)) is reached for a no-import bare name, an AMBIGUOUS
//       multi-target same-name `use` (`UseIndex::unique_path` is `None` for >1),
//       AND a unique SINGLE-SEGMENT self-import (`use Foo;` — the recursion guard
//       skips arm (b) when the import path is the bare name itself); and the
//       use-binding CHAIN (`resolve_use_binding_chain`) returns the FIRST
//       accessible target that resolves, not a single proven one. A qualified
//       UNROOTED unshadowed path likewise raw-suffix matches a collected unique
//       token (`candidate_matches`'s direct arm). All of these land on a
//       uniquely-named sanctioned token's single genuine def (no decoy to forge).
//     Do NOT run further incremental resolver-hardening passes for this guard,
//     and do NOT extend this disclosure per-instance — the two classes above are
//     the complete characterization. This classifier is revisited only if it is
//     promoted from residual guard to primary enforcement, or if a real
//     production sink reference starts using one of these residual shapes.
//   - TRANSITIVE on BOTH sides: a `TypeExpr`-bearing OUTPUT is decided by the
//     field-closure over RESOLVED ids; a FORGEABLE INPUT is the same field-closure
//     (a wrapper of a raw surface/member seed is caught). The dual-bearing
//     defense is "DIRECT carve-out + TRANSITIVE tripwire": the
//     forgeable-closure's bearing fence keeps a DUAL-BEARING wrapper that
//     DIRECTLY co-holds a resolution-authority seed (the carve-out stays DIRECT —
//     the 20-FP fence), while the soundness TRIPWIRE
//     (`forgeable_input_fence_has_no_dual_bearing_type`) uses a TRANSITIVE
//     raw-authority reach for its seed side (a DIRECT `TypeExpr` field + a
//     transitive reach to an RA-seed fires — the tripwire needs soundness, not
//     FP-freedom, so any hit is investigated).
//   - FAIL-CLOSED on BOTH sides: an unclassifiable PascalCase OUTPUT ident
//     (`unclassifiable_output_idents`) AND an unclassifiable PascalCase INPUT
//     ident on a bearing boundary (`unclassifiable_input_idents`) both FAIL — an
//     unread DTO/wrapper, or a forged-qualifier bare name, surfaces loudly rather
//     than passing as a benign leaf. The non-authority exemptions are QUALIFIED
//     `(module, name)` (anti-vacuity-checked) or non-field-bearing CATEGORY
//     entries (trait bound / generic-or-assoc / non-collected external) carrying
//     APPROVED qualified homes — the category match is QUALIFIER-AWARE (it
//     consults the `Unresolved` ref's PATH + the collision index, never the bare
//     final segment): a multi-segment ref is exempt only when its qualifier
//     matches an approved home (a forged `evil::Span` FIRES), a one-segment
//     generic/assoc name is benign, and a one-segment trait-bound / external is
//     exempt only when no same-name def is collected (fail closed on the
//     ambiguity). The dual-bearing tripwire's sanctioned-carrier exemption is
//     likewise QUALIFIED `(module, name)` (a wrong-module same-name token FIRES).
//   - POLICY-ADMITTED vs CONSTRUCTION-CHAIN split: only the policy-admitted
//     publication token + the per-framework sealed tokens
//     (`POLICY_ADMITTED_SAFE_INPUTS`) are safe sink-fn inputs; a PRE-admission
//     construction-chain struct (`SEALED_CONSTRUCTION_CHAIN_STRUCTS`) taken
//     directly fires (it bypassed the policy gate).
//   - INLINE-MOD-AWARE: the sink-fn collector qualifies an inline-`mod` fn under
//     `<file>::inner` (a module-path stack), so a `(module, fn)` allowlist entry
//     is precise per inline submodule.
// SCOPE: this is the Kind-A / PUBLICATION boundary, NOT a claim that no
// raw-node→`TypeExpr` boundary remains ANYWHERE — the named Kind-B
// raise-then-decide residual is deferred to 8-A4 and pinned separately (see the
// `kind_b_raise_then_decide_entrypoints_pinned` section above).
//
// scanner_invariant: cross_sink_raw_authority_to_typeexpr
// scanner_justification: the transitive DTO-content + forgeable-input PAIRING across multiple sink modules is not expressible as a single Rust visibility / type-state check — the sealed admitted-token chain (private fields + private Seal) is the compiler primary, this scanner is the residual cross-module pairing supplement.
// mechanism_ruling: 8a3-binding-ruling-2026-06-24
// hardening_rounds: 0
// hardening_history: adoption — replaces the name-based fix5 closed-allowlist output_sink boundary guard with a structural field-closure cross-sink transitive guard. STRUCTURAL REFINEMENT (NOT a hardening round, NOT spelling additions — the guard was made genuinely structural + fail-closed per the guard-mechanism ruling 8a3-guard-mechanism-consult-2026-06-24). A FOLLOW-UP STRUCTURAL-CORRECTNESS refinement then carried GENUINE module-qualified `(module, name)` `TypeDefId` identity through the closure graph with a CONSERVATIVE FAIL-CLOSED resolver, REPLACING the prior final-segment matching (which the prior claim called "module-qualified" but implemented only as bare-final-segment with a side `def_modules` index, so the two `IndexSignature` defs MERGED): the two `IndexSignature` defs are now DISTINCT ids; the safe-input collision check became pure anti-vacuity (the bearing-gated same-name carve-out was deleted); the dual-bearing tripwire seed side went TRANSITIVE while the forgeable carve-out stays DIRECT; and the non-authority input exemptions became qualified + anti-vacuity-checked. A LATER structural-correctness completion then closed three residual fail-OPEN spots a follow-up review found, where the qualified-identity model was claimed but not yet enforced on a bare-name / unique-name basis: (1) the qualified-path resolver arm resolved a UNIQUE final-segment candidate purely by uniqueness, ignoring the written qualifier — now a qualified path direct-matches only when the candidate's module is a suffix-or-equal of the qualifier OR is a proven `pub`/`pub(crate)` re-export of it (a cross-file re-export index + relative-qualifier normalization keep genuine re-exports resolving); (2) the input/output completeness category exemptions matched by BARE final segment — now QUALIFIER-AWARE (the category carries approved homes; a multi-segment ref must match one, a one-segment trait-bound/external is exempt only with no same-name collected def); (3) the dual-bearing tripwire's sanctioned-carrier exemption matched by BARE name — now the QUALIFIED `policy_admitted_safe_input_ids ∪ sealed_construction_chain_ids` set. A FINAL identity-correctness completion then closed the four remaining resolver-permissiveness spots a follow-up review found, where the "conservative fail-closed / module-qualified" claim was not yet fully true: (a) a too-short ANCESTOR prefix was accepted as a direct qualifier match (`crate::X` prefix-matched a deep real module) — direct matching is now SUFFIX-OR-EQUAL only, so ancestor-shortened / relative re-export references resolve through the proven `pub`/`pub(crate)` re-export rail ALONE (now genuinely load-bearing, keyed by the NORMALIZED absolute written path so a `super::…` re-export reference resolves); (b) a unique import whose target was external/unprovable fell through to the unique-collected-def shortcut (`use external::X as AdmittedPublishedMember` blessed the sanctioned token by uniqueness) — the import-shadow now PRESERVES the Unresolved import path; (c) the re-export index recorded ANY `Restricted` visibility incl `pub(self)`/`pub(in …)` — now `pub`/`pub(crate)` ONLY (a narrow scoped re-export is not a crate-wide proof other modules can write); (d) a `super` could pop the crate root, leaving a loosely-matching bare path — `super` now fails closed before escaping above the root. A SUBSEQUENT follow-up review found the resolver STILL resolved unproven identities in three same-class spots, all defense-in-depth (the production sealed-token primary is sound and untouched): (1) the import-shadow arm, after an import target failed to resolve, fell through to a UNIQUENESS shortcut for an intra-crate NON-RENAMED import (so a forged `use crate::evil::AdmittedPublishedMember` still resolved to the unique real token); (2) the re-export prover compared the re-export TARGET module by SUFFIX-or-equal (so a single-segment `pub use publication_authority::X` proved any `…::publication_authority` home); (3) an UNROOTED qualifier was accepted on a raw suffix without consulting the file's `use`-index (so a `use`-shadowed first segment, `use crate::other as publication_authority` then `publication_authority::X`, resolved to the safe token). These tightened the common paths: the resolver resolves a written reference by own-module-def, a genuine `pub`/`pub(crate)` re-export, a proven intra-crate `use`-binding chain (a module-scoped use-binding graph — narrow, intra-crate-only, non-glob, module/descendant-visibility, cycle-bounded; an unsupported `use` form contributes no binding => Unresolved), or a proven (suffix-or-equal DIRECT, EXACT-target re-export) qualifier; the uniqueness fall-through after an unresolved UNIQUE import is removed; the re-export TARGET match is EXACT; and an unrooted qualifier whose first segment the file `use`-shadows is re-resolved through the shadow. The genuine private chain (`registry_decl`'s `super::ResolvedTypeDeclaration` through the parent module's private `use`) resolves by PROOF; the forged `crate::evil::AdmittedPublishedMember` (a rooted qualifier naming no def-home) stays Unresolved. TERMINAL DISPOSITION (architect ruling `8a3i2-consult-8020-terminal`, 2026-06-24): this is an 80/20 fail-closed identity classifier for the current production reference shapes, NOT a complete Rust name resolver, and it CLEARS the acceptance bar for its defense-in-depth role behind the compiler-enforced sealed-token primary. Four residual forged shapes are ACCEPTED debt (cfg/cfg_attr-gated `use` indexed without active-cfg eval; ambiguous multi-import falling through to global uniqueness; unrooted-unshadowed raw-suffix match; no-import bare-unique global-uniqueness) — adversarial relative to this crate's common production sink-token references (each sanctioned token is uniquely named), recorded in `docs/arch/output-projector-residual-guard-debt.md`; no further incremental resolver-hardening passes are run for this guard. hardening_rounds stays 0 (identity-correctness work, not spelling/evasion increments).
// ===========================================================================

/// Output-authority SEEDs as MODULE-QUALIFIED `(module, name)` IDs: a return /
/// out-param transitively reaching any of these is "TypeExpr-bearing". `TypeExpr`
/// is the closure ROOT; the rest are additional roots so a type reaching a
/// published DTO is flagged even when the closure to `TypeExpr` would pass
/// through a type def the collector did not parse (cross-crate boundary
/// robustness — the seed ID is canonical whether or not its home file is read).
/// This list is the self-test's EXPECTED-flag oracle, NOT a closed spelled-name
/// allowlist — the field-closure flags newly-named DTOs without an entry here.
///
/// Each seed names its CANONICAL home module so the closure seeds by `TypeDefId`,
/// not bare name. `ResolvedMacroPayload` here is the BEARING `results` alias
/// (`ResolvedOutcome<Arc<MacroSurfaceDtos>>`), DISTINCT from the sealed
/// publication-authority construction-chain token of the same bare name.
const OUTPUT_AUTHORITY_SEEDS: &[(&str, &str)] = &[
    ("verter_type_expr", "TypeExpr"),
    (
        "verter_semantic::analysis::type_expand::request",
        "ExpandedField",
    ),
    (
        "verter_semantic::analysis::type_expand::request",
        "ExpandedIndexSignature",
    ),
    (
        "verter_semantic::analysis::type_expand::request",
        "ExpandedObjectShape",
    ),
    (
        "verter_semantic::analysis::type_expand::request",
        "ExpandedProperty",
    ),
    (
        "verter_semantic::analysis::type_expand::request",
        "ExpandedCallSignature",
    ),
    ("verter_semantic::analysis::types", "AnalyzedPropField"),
    ("verter_semantic::analysis::types", "AnalyzedEmitField"),
    ("verter_semantic::analysis::types", "AnalyzedSlotField"),
    ("verter_semantic::analysis::types", "AnalyzedExposeField"),
    (
        "crate::typeinfo::framework_surface::results",
        "NamedTypeMember",
    ),
    (
        "crate::typeinfo::framework_surface::results",
        "MacroSurfaceDtos",
    ),
    (
        "verter_semantic::analysis::type_solver::query_engine",
        "ProjectedSurface",
    ),
    (
        "verter_semantic::analysis::type_solver::query_engine",
        "ProjectedMember",
    ),
    (
        "verter_semantic::analysis::type_solver::query_engine",
        "ProjectedIndexSignature",
    ),
    (
        "crate::typeinfo::framework_surface::results",
        "PropsSurface",
    ),
    (
        "crate::typeinfo::framework_surface::results",
        "EmitsSurface",
    ),
    (
        "crate::typeinfo::framework_surface::results",
        "OptionsSurface",
    ),
    (
        "crate::typeinfo::framework_surface::results",
        "ExposeSurface",
    ),
    (
        "crate::typeinfo::framework_surface::results",
        "ModelSurface",
    ),
    (
        "crate::typeinfo::framework_surface::results",
        "ResolvedMacroPayload",
    ),
    (
        "crate::typeinfo::framework_surface::results",
        "NormalizedSurface",
    ),
    (
        "crate::typeinfo::framework_surface::results",
        "NormalizedSurfaces",
    ),
];

/// Input-authority SEEDs as MODULE-QUALIFIED `(module, name)` IDs: a fn param of
/// any of these (or a wrapper / alias transitively reaching one) is "forgeable
/// raw authority". The admitted tokens (`AdmittedPublishedMember`,
/// `ResolvedPayloadSurface`, `SurfaceMemberCandidate`, the projector
/// `ResolvedMacroPayload`, the framework-surface `ResolvedVueSurface`) are NOT
/// here — they are the ALLOWED input, so a fn taking only admitted tokens has
/// zero forgeable inputs and never fires.
///
/// `IndexSignature` here is the AUTHORITY one — `crate::semantic_query::
/// IndexSignature`, whose `key_type` / `value_type` fields are `SemanticNodeId`
/// (raw graph handles). It is DISTINCT from `verter_type_expr::IndexSignature`
/// (whose `key_type` / `value_type` are `TypeExpr` — an already-lowered-IR
/// holder, an OUTPUT-side bearing leaf, NOT a forgeable raw-authority input).
const INPUT_AUTHORITY_SEEDS: &[(&str, &str)] = &[
    ("crate::semantic_query", "SemanticNodeId"),
    ("crate::semantic_query", "SurfaceMember"),
    ("crate::meta_resolve::projection_demand", "ProjectionCursor"),
    ("crate::semantic_query", "SurfaceView"),
    ("crate::typeinfo::surface", "TypeInfoSurfaceMember"),
    ("crate::typeinfo::surface", "TypeInfoSurfaceSignature"),
    ("crate::typeinfo::surface", "TypeInfoIndexSignature"),
    ("crate::semantic_query", "IndexSignature"),
    ("crate::typeinfo::surface", "TypeInfoSurface"),
    (
        "crate::typeinfo::framework_surface::vue_exec",
        "VueMacroSurface",
    ),
];

/// The output-authority seeds as canonical `TypeDefId`s (the bearing-closure
/// roots).
fn output_authority_seed_ids() -> std::collections::BTreeSet<TypeDefId> {
    OUTPUT_AUTHORITY_SEEDS
        .iter()
        .map(|(m, n)| TypeDefId::new(*m, *n))
        .collect()
}

/// The input-authority seeds as canonical `TypeDefId`s (the forgeable-closure
/// roots).
fn input_authority_seed_ids() -> std::collections::BTreeSet<TypeDefId> {
    INPUT_AUTHORITY_SEEDS
        .iter()
        .map(|(m, n)| TypeDefId::new(*m, *n))
        .collect()
}

/// Fold the canonical KNOWN `TypeDefId`s (output + input seeds + the
/// policy-admitted safe-input tokens + the sealed construction-chain structs)
/// into a [`NameDefIndex`] so a reference NAMING one resolves to its CANONICAL
/// id even when its home file is not a read root (cross-crate robustness). When
/// the home IS read, the canonical id equals the collected id (same module ⇒ one
/// candidate), so an unqualified ref to a non-colliding known name still
/// resolves. A name colliding with a DIFFERENT collected module (`IndexSignature`,
/// `ResolvedMacroPayload`) yields TWO candidates, so an UNQUALIFIED ref to it is
/// AMBIGUOUS (fail-closed) while a QUALIFIED ref picks the right one — the crux
/// distinction.
fn name_index_with_seed_ids(base: &NameDefIndex) -> NameDefIndex {
    let mut idx = base.clone();
    for id in output_authority_seed_ids()
        .into_iter()
        .chain(input_authority_seed_ids())
        .chain(policy_admitted_safe_input_ids())
        .chain(sealed_construction_chain_ids())
    {
        idx.entry(id.name.clone()).or_default().insert(id);
    }
    idx
}

/// Module-path prefixes (`::`-joined) of the registered output sinks this guard
/// scans. A production `.rs` file whose module path (derived from its
/// crate-relative path) starts with one of these prefixes is in scope.
const SINK_SCAN_PREFIXES: &[&str] = &[
    "crate::meta_resolve::projectors",
    "crate::meta_resolve::materialize",
    "crate::component_meta_caches",
    "crate::typeinfo::framework_surface",
    "crate::resolver_core::component_meta_query_engine",
    "crate::project_semantic_dispatch::raise",
];

/// Closed allowlist of the GENUINE sink-local APIs that legitimately pair a
/// forgeable raw-authority input with a `TypeExpr`-bearing output. Keyed
/// `(module_path, fn_name)` so an API of the same name in a DIFFERENT module is
/// NOT silently allowed. Each entry is a sink-internal raiser / projector /
/// candidate-reader that the sealed admitted-token chain routes through.
///
/// This is NOT a DTO-name allowlist (the anti-recurrence property forbids that)
/// — it is the closed set of sink-local FUNCTIONS the compiler-enforced token
/// chain depends on. A NEW non-allowlisted fn pairing forgeable authority with
/// a TypeExpr output FIRES default-deny.
const SINK_LOCAL_RAW_AUTHORITY_ALLOWLIST: &[(&str, &str)] = &[
    // The canonical `SemanticNodeId → TypeExpr` raiser + its sealed-carrier seam
    // and the interim Kind-B bridge (both pinned by their own guards).
    (
        "crate::project_semantic_dispatch::raise",
        "raise_node_to_type_expr",
    ),
    (
        "crate::project_semantic_dispatch::raise",
        "raise_node_to_type_expr_inner",
    ),
    (
        "crate::project_semantic_dispatch::raise",
        "raise_surface_view_to_object_type_expr",
    ),
    (
        "crate::project_semantic_dispatch::raise",
        "output_shell_raise_sealed",
    ),
    (
        "crate::project_semantic_dispatch::raise",
        "legacy_semantic_type_expr_bridge",
    ),
    (
        "crate::project_semantic_dispatch::raise",
        "raise_and_reduce_with_context",
    ),
    (
        "crate::project_semantic_dispatch::raise",
        "materialize_type_expr",
    ),
    (
        "crate::project_semantic_dispatch::raise",
        "materialize_output_type_expr_for_test",
    ),
    (
        "crate::project_semantic_dispatch::raise",
        "materialize_reduced_output_type_expr_for_test",
    ),
    // The query-engine surface projector — sink-internal, confined to the
    // `component_meta_query_engine` subtree (the forgeable input is the leak the
    // re-export removal closes; these stay reachable only in-subtree).
    (
        "crate::resolver_core::component_meta_query_engine::surface",
        "surface_view_to_projected_surface",
    ),
    (
        "crate::resolver_core::component_meta_query_engine::surface",
        "projected_surface_from_semantic_node",
    ),
    (
        "crate::resolver_core::component_meta_query_engine::surface",
        "projected_surface_from_semantic_node_inner",
    ),
    (
        "crate::resolver_core::component_meta_query_engine::surface",
        "projected_compound_root_surface_via_dispatch",
    ),
    // The framework-surface member raisers — confined to `vue_exec`, reachable
    // only through a token-gated normalizer.
    (
        "crate::typeinfo::framework_surface::vue_exec",
        "raise_member_value",
    ),
    (
        "crate::typeinfo::framework_surface::vue_exec",
        "raise_realized_callable_member_value",
    ),
    // The Svelte-specific callback-events normalizer — a PRIVATE svelte fn fed a
    // resolution-derived `TypeInfoSurface` (from `navigate_param_to_object_surface`),
    // not a caller-forged surface; reachable only within `svelte_exec`.
    (
        "crate::typeinfo::framework_surface::svelte_exec",
        "callback_events_from_props_surface",
    ),
    // The SANCTIONED token-MINTING projector callers. Each keeps a
    // `ProjectionCursor` demand and resolves + ADMITS internally (through
    // `resolve_macro_payload`/`resolve_payload_surface`/`read_surface_members` +
    // `admit_published_member`), then publishes via the token-gated
    // `surface_member_to_expanded_field`. Their `Vec<ExpandedField>` /
    // `Option<ExpandedField>` return is the aggregate of token-gated
    // materialisations — the cursor is the demand spec, NOT a reverse-
    // materialisation vector. (Per the ruling's GREEN list: "root APIs keeping a
    // root cursor that resolve/admit internally + are allowlisted".)
    ("crate::meta_resolve::projectors::props", "project_props"),
    ("crate::meta_resolve::projectors::emits", "project_emits"),
    (
        "crate::meta_resolve::projectors::options",
        "project_options",
    ),
    ("crate::meta_resolve::projectors::slots", "project_slots"),
    (
        "crate::meta_resolve::projectors::exposed",
        "project_exposed",
    ),
    (
        "crate::meta_resolve::projectors::output_sink",
        "project_model",
    ),
    // Sink-internal node→shape raisers/reducers: each reduces a `SemanticNodeId`
    // the sink resolved (never a caller-forged subject paired with a cursor) to
    // a sealed `MaterializedOutputTypeExpr` / shape. The cap mint + carrier
    // unwrap stay inside the sink (pinned by the mint-scope guard); these are
    // the shared graph-native reducers the publication path routes through.
    (
        "crate::meta_resolve::materialize::field_types",
        "reduce_member_value_graph_native_with_context",
    ),
    (
        "crate::meta_resolve::projectors::output_sink",
        "shell_raise_to_type_expr",
    ),
    (
        "crate::project_semantic_dispatch::raise",
        "index_key_to_type_expr",
    ),
];

/// All idents referenced in a `syn::Type`'s token stream (the type names a
/// field / param / return type mentions). Comment/string-blind by construction
/// (a `syn` token stream carries no comments; a literal is a `Literal`, never
/// an `Ident`). Used by the token-construction guard helpers, which match a
/// token name appearing ANYWHERE in a type (a qualified path still contains the
/// bare ident), so the flat ident set is the right shape there.
fn type_idents(tokens: &proc_macro2::TokenStream) -> Vec<String> {
    let mut out = Vec::new();
    fn walk(ts: &proc_macro2::TokenStream, out: &mut Vec<String>) {
        for tt in ts.clone() {
            match tt {
                proc_macro2::TokenTree::Ident(id) => out.push(id.to_string()),
                proc_macro2::TokenTree::Group(g) => walk(&g.stream(), out),
                _ => {}
            }
        }
    }
    walk(tokens, &mut out);
    out
}

/// MODULE-QUALIFIED raw references: walk a `syn::Type` and produce one ORDERED
/// PATH (`["crate","semantic_query","SemanticNodeId"]` / `["IndexSignature"]`)
/// per NAMED type it references, recursing into generic args / tuple / ref /
/// slice / array / ptr / dyn / impl as SEPARATE refs. Unlike a bare-final-segment
/// flatten, this RETAINS
/// the full path of each reference so the conservative resolver can attempt
/// `(module, name)` qualification — a `crate::semantic_query::IndexSignature`
/// field yields the path `["crate","semantic_query","IndexSignature"]`, which
/// the resolver disambiguates from `verter_type_expr::IndexSignature`.
fn type_segment_refs(ty: &syn::Type) -> Vec<Vec<String>> {
    let mut out = Vec::new();
    collect_type_segment_refs(ty, &mut out);
    out
}

fn collect_type_segment_refs(ty: &syn::Type, out: &mut Vec<Vec<String>>) {
    match ty {
        syn::Type::Path(tp) => {
            if let Some(qself) = &tp.qself {
                collect_type_segment_refs(&qself.ty, out);
            }
            // The full path as written — every segment ident (module qualifiers
            // + the terminal type segment). The resolver matches the terminal
            // segment as the NAME and the leading segments as a module suffix.
            let path: Vec<String> = tp
                .path
                .segments
                .iter()
                .map(|s| s.ident.to_string())
                .collect();
            if !path.is_empty() {
                out.push(path);
            }
            // Recurse into the FINAL segment's generic args as SEPARATE refs.
            if let Some(last) = tp.path.segments.last() {
                if let syn::PathArguments::AngleBracketed(args) = &last.arguments {
                    for arg in &args.args {
                        if let syn::GenericArgument::Type(inner) = arg {
                            collect_type_segment_refs(inner, out);
                        }
                    }
                } else if let syn::PathArguments::Parenthesized(p) = &last.arguments {
                    for inp in &p.inputs {
                        collect_type_segment_refs(inp, out);
                    }
                    if let syn::ReturnType::Type(_, rt) = &p.output {
                        collect_type_segment_refs(rt, out);
                    }
                }
            }
        }
        syn::Type::Reference(r) => collect_type_segment_refs(&r.elem, out),
        syn::Type::Ptr(p) => collect_type_segment_refs(&p.elem, out),
        syn::Type::Slice(s) => collect_type_segment_refs(&s.elem, out),
        syn::Type::Array(a) => collect_type_segment_refs(&a.elem, out),
        syn::Type::Paren(p) => collect_type_segment_refs(&p.elem, out),
        syn::Type::Group(g) => collect_type_segment_refs(&g.elem, out),
        syn::Type::Tuple(t) => {
            for elem in &t.elems {
                collect_type_segment_refs(elem, out);
            }
        }
        syn::Type::TraitObject(to) => {
            for bound in &to.bounds {
                if let syn::TypeParamBound::Trait(tb) = bound {
                    let path: Vec<String> = tb
                        .path
                        .segments
                        .iter()
                        .map(|s| s.ident.to_string())
                        .collect();
                    if !path.is_empty() {
                        out.push(path);
                    }
                }
            }
        }
        syn::Type::ImplTrait(it) => {
            for bound in &it.bounds {
                if let syn::TypeParamBound::Trait(tb) = bound {
                    let path: Vec<String> = tb
                        .path
                        .segments
                        .iter()
                        .map(|s| s.ident.to_string())
                        .collect();
                    if !path.is_empty() {
                        out.push(path);
                    }
                }
            }
        }
        _ => {}
    }
}

/// A file-scoped `use`-import index: the imported name (the alias if `use X as
/// Alias`, else the final segment) mapped to the FULL imported path. Feeds the
/// resolver so an unqualified ref to an imported / aliased name resolves to the
/// import's qualified target (`use crate::semantic_query::SemanticNodeId as
/// NodeId;` then `struct W(NodeId)` maps `NodeId` →
/// `crate::semantic_query::SemanticNodeId`).
#[derive(Debug, Default, Clone)]
struct UseIndex {
    /// `imported-name -> {full path segments}` (multiple paths under one name
    /// makes that name AMBIGUOUS, so the resolver does not use it to qualify).
    imports: BTreeMap<String, std::collections::BTreeSet<Vec<String>>>,
}

impl UseIndex {
    fn add(&mut self, name: String, path: Vec<String>) {
        self.imports.entry(name).or_default().insert(path);
    }

    /// The single qualified path imported under `name`, if exactly one.
    fn unique_path(&self, name: &str) -> Option<&Vec<String>> {
        let set = self.imports.get(name)?;
        if set.len() == 1 {
            set.iter().next()
        } else {
            None
        }
    }
}

/// Collect a file's `use` items into a [`UseIndex`], walking nested
/// `use a::{b, c as d}` trees and `UseRename` (`use … as Alias`). A glob
/// (`use x::*`) imports no specific name and is skipped (it cannot disambiguate
/// a single target).
fn collect_use_index(file: &syn::File) -> UseIndex {
    struct V {
        idx: UseIndex,
    }
    fn walk_tree(tree: &syn::UseTree, prefix: &mut Vec<String>, idx: &mut UseIndex) {
        match tree {
            syn::UseTree::Path(p) => {
                prefix.push(p.ident.to_string());
                walk_tree(&p.tree, prefix, idx);
                prefix.pop();
            }
            syn::UseTree::Name(n) => {
                let name = n.ident.to_string();
                let mut path = prefix.clone();
                path.push(name.clone());
                idx.add(name, path);
            }
            syn::UseTree::Rename(r) => {
                let mut path = prefix.clone();
                path.push(r.ident.to_string());
                idx.add(r.rename.to_string(), path);
            }
            syn::UseTree::Group(g) => {
                for item in &g.items {
                    walk_tree(item, prefix, idx);
                }
            }
            syn::UseTree::Glob(_) => {}
        }
    }
    impl<'ast> syn::visit::Visit<'ast> for V {
        fn visit_item_use(&mut self, u: &'ast syn::ItemUse) {
            let mut prefix = Vec::new();
            walk_tree(&u.tree, &mut prefix, &mut self.idx);
        }
    }
    let mut v = V {
        idx: UseIndex::default(),
    };
    syn::visit::Visit::visit_file(&mut v, file);
    v.idx
}

/// MODULE-QUALIFIED type identity — the `(module, name)` pair that is the
/// closure-graph node identity. Two types sharing a bare final segment but
/// DEFINED in different modules (`crate::semantic_query::IndexSignature`, which
/// carries `SemanticNodeId` fields, vs `verter_type_expr::IndexSignature`, which
/// carries `TypeExpr` fields) are DISTINCT `TypeDefId`s — they are NOT merged
/// into one bare-name slot. This is what makes the bearing / forgeable closures
/// operate on genuine type identity rather than final-segment matching: a
/// wrapper of the authority `IndexSignature` cannot become bearing through a
/// merged edge to the type-expr `IndexSignature`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct TypeDefId {
    module: String,
    name: String,
}

impl TypeDefId {
    fn new(module: impl Into<String>, name: impl Into<String>) -> Self {
        TypeDefId {
            module: module.into(),
            name: name.into(),
        }
    }
}

impl std::fmt::Display for TypeDefId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}::{}", self.module, self.name)
    }
}

/// A reference carrier the def-graph and sink signatures carry instead of a bare
/// ident. A reference RESOLVES to a concrete `TypeDefId` when the conservative
/// resolver can prove a single target; otherwise it stays `Unresolved` and the
/// boundary / seed consumers apply FAIL-CLOSED rules (an unresolved/ambiguous
/// reference at a TypeExpr-bearing sink boundary or a closure seed path that is
/// not a known container / std primitive / justified non-authority FAILS — the
/// scanner cannot prove the type is safe).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum TypeRef {
    /// The reference resolved to exactly one concrete module-qualified def.
    Resolved(TypeDefId),
    /// The reference could not be resolved to a single def (ambiguous across
    /// modules with no local/import disambiguation, or un-locatable). The
    /// referencing module + the path's final segment are retained so the
    /// boundary consumers can classify it (a known container / std primitive /
    /// justified non-authority external is benign; anything else fails closed).
    Unresolved {
        from_module: String,
        /// The full `::`-segment path as written (`["crate","semantic_query",
        /// "SemanticNodeId"]` or `["IndexSignature"]`).
        path: Vec<String>,
        /// The path's final (type-naming) segment.
        final_segment: String,
    },
}

impl TypeRef {
    /// The final type-naming segment regardless of resolution state — used by the
    /// boundary completeness checks to classify a reference by its bare name
    /// (container / non-authority / safe-token) before deciding fail-closed.
    fn final_segment(&self) -> &str {
        match self {
            TypeRef::Resolved(id) => &id.name,
            TypeRef::Unresolved { final_segment, .. } => final_segment,
        }
    }

    /// The resolved `TypeDefId`, if this reference resolved.
    fn resolved(&self) -> Option<&TypeDefId> {
        match self {
            TypeRef::Resolved(id) => Some(id),
            TypeRef::Unresolved { .. } => None,
        }
    }
}

/// A collected type definition's outgoing type references — one [`TypeRef`] per
/// named type its fields / variant-fields / alias-RHS reference. The bearing /
/// forgeable closures follow these edges by RESOLVED `TypeDefId` (an
/// `Unresolved` edge never propagates membership — it is conservatively a
/// non-edge, and is surfaced fail-closed only when it sits on a sink boundary
/// or a seed path).
#[derive(Debug, Default, Clone)]
struct TypeDefRefs {
    /// Every type the def references, each resolved (or left unresolved) by the
    /// conservative resolver.
    refs: std::collections::BTreeSet<TypeRef>,
}

/// `syn` visitor collecting `TypeDefId -> {raw outgoing reference paths}` for
/// every struct / enum / type-alias in the parsed files. Each definition is
/// recorded under its MODULE-QUALIFIED identity (the file's module base, pushed
/// with each inline `mod`), and each outgoing reference is the FULL path as
/// written (via [`type_segment_refs`]) so the conservative resolver can later
/// disambiguate `(module, name)` identity. The collector ALSO retains the file's
/// `use`-import index so the resolver can resolve an unqualified / aliased
/// reference through the file's imports.
///
/// Generic param NAMES (e.g. `T`) are collected as one-segment paths — harmless,
/// since a bare type param resolves to nothing (no def, no import) and stays a
/// benign `Unresolved` the closure never propagates and the boundary checks
/// skip (a single-char ident is never PascalCase-classifiable).
struct TypeDefCollector {
    /// `TypeDefId -> the raw reference paths` (full `::` segments per referenced
    /// type), pre-resolution. [`collect_type_defs`] resolves these into
    /// [`TypeDefRefs`].
    raw_defs: BTreeMap<TypeDefId, Vec<Vec<String>>>,
    /// `alias TypeDefId -> the RHS path` for a `type X = <path>;` whose RHS is a
    /// SINGLE named type (the re-export-alias case). Used to collapse a same-name
    /// re-export alias (`pub type ResolvedTypeDeclaration =
    /// crate::resolver_core::ResolvedTypeDeclaration;`) onto the real def so it is
    /// not a spurious cross-module collision.
    alias_targets: BTreeMap<TypeDefId, Vec<String>>,
    /// The module-path stack: the file's base module path, pushed with each
    /// inline `mod ident {}` entered.
    module_stack: Vec<String>,
}

impl Default for TypeDefCollector {
    fn default() -> Self {
        TypeDefCollector {
            raw_defs: BTreeMap::new(),
            alias_targets: BTreeMap::new(),
            module_stack: vec!["crate".to_string()],
        }
    }
}

impl TypeDefCollector {
    /// Start a fresh collector rooted at a specific file's module path (so each
    /// definition is recorded under its module-qualified identity).
    fn with_module_base(base: String) -> Self {
        TypeDefCollector {
            raw_defs: BTreeMap::new(),
            alias_targets: BTreeMap::new(),
            module_stack: vec![base],
        }
    }

    fn current_module(&self) -> String {
        self.module_stack
            .last()
            .cloned()
            .unwrap_or_else(|| "crate".to_string())
    }

    fn record_def(&mut self, name: String, refs: Vec<Vec<String>>) {
        let id = TypeDefId::new(self.current_module(), name);
        let entry = self.raw_defs.entry(id).or_default();
        entry.extend(refs);
    }
}

impl<'ast> syn::visit::Visit<'ast> for TypeDefCollector {
    fn visit_item_struct(&mut self, s: &'ast syn::ItemStruct) {
        let refs: Vec<Vec<String>> = s
            .fields
            .iter()
            .flat_map(|field| type_segment_refs(&field.ty))
            .collect();
        self.record_def(s.ident.to_string(), refs);
        syn::visit::visit_item_struct(self, s);
    }

    fn visit_item_enum(&mut self, e: &'ast syn::ItemEnum) {
        let refs: Vec<Vec<String>> = e
            .variants
            .iter()
            .flat_map(|variant| &variant.fields)
            .flat_map(|field| type_segment_refs(&field.ty))
            .collect();
        self.record_def(e.ident.to_string(), refs);
        syn::visit::visit_item_enum(self, e);
    }

    fn visit_item_type(&mut self, t: &'ast syn::ItemType) {
        let refs = type_segment_refs(&t.ty);
        // Record a SINGLE-named-type alias RHS so a same-name re-export alias can
        // be collapsed onto its target (it is not a distinct cross-module type).
        if refs.len() == 1 {
            self.alias_targets.insert(
                TypeDefId::new(self.current_module(), t.ident.to_string()),
                refs[0].clone(),
            );
        }
        self.record_def(t.ident.to_string(), refs);
        syn::visit::visit_item_type(self, t);
    }

    fn visit_item_mod(&mut self, m: &'ast syn::ItemMod) {
        if mod_is_cfg_test(&m.attrs) {
            return; // test submodule — production-unreachable
        }
        // An inline `mod ident {}` qualifies its inner defs as `<parent>::ident`,
        // so a same-named type in a different inline submodule does not collide.
        let child = format!("{}::{}", self.current_module(), m.ident);
        self.module_stack.push(child);
        syn::visit::visit_item_mod(self, m);
        self.module_stack.pop();
    }
}

/// Crate-relative files whose type definitions the field-closure reads: ALL of
/// `verter_session/src` plus the cross-crate output-seed homes (the seed types
/// whose defs live OUTSIDE `verter_session`). Reading those external defs is
/// what lets the closure follow a `verter_session` DTO that wraps a
/// `verter_semantic` `ProjectedSurface` down to `TypeExpr`.
///
/// Returns `(module_base, src)` pairs — the module base is the `::`-joined
/// module path the file's top-level defs live under, so the collector records
/// each definition under its MODULE-QUALIFIED identity (the bare-name-collision
/// detector + the qualified safe-input exemption). Cross-crate files use a
/// `<crate-name>::…` synthetic base derived from the relative path.
fn type_def_source_files() -> Vec<(String, String)> {
    let mut srcs: Vec<(String, String)> = production_src_files()
        .into_iter()
        .map(|(rel, src)| (module_path_for_rel(&rel), src))
        .collect();
    // Cross-crate seed homes (relative to THIS crate's manifest dir). The
    // synthetic module base names the owning crate so a cross-crate type's
    // qualified identity never collides with a `verter_session` `crate::…` one.
    const EXTERNAL: &[(&str, &str)] = &[
        ("../verter_type_expr/src/lib.rs", "verter_type_expr"),
        (
            "../verter_semantic/src/analysis/type_solver/query_engine.rs",
            "verter_semantic::analysis::type_solver::query_engine",
        ),
        (
            "../verter_semantic/src/analysis/type_expand/request.rs",
            "verter_semantic::analysis::type_expand::request",
        ),
        (
            "../verter_semantic/src/analysis/types.rs",
            "verter_semantic::analysis::types",
        ),
        // `PreparedTypeDecl { body: TypeExpr, merged_contributors: Vec<TypeExpr>, … }`
        // is a TypeExpr-BEARING DTO returned by a sink fn (`prepared_type_decl`);
        // reading its home lets the closure classify it as bearing (the
        // fail-closed completeness check surfaced it as otherwise-unread).
        (
            "../verter_semantic/src/analysis/type_solver/prepared.rs",
            "verter_semantic::analysis::type_solver::prepared",
        ),
    ];
    for (rel, module_base) in EXTERNAL {
        let path = crate_root().join(rel);
        if let Ok(src) = std::fs::read_to_string(&path) {
            srcs.push((module_base.to_string(), src));
        }
    }
    srcs
}

/// `name -> the set of MODULE-QUALIFIED def IDs that define a type of that name`
/// — the collision index. A name mapping to MORE THAN ONE `TypeDefId` is a
/// cross-module collision (`IndexSignature` in `crate::semantic_query` AND
/// `verter_type_expr`). Built from every collected def's identity, so the
/// resolver and the collision check operate on genuine `(module, name)` identity.
type NameDefIndex = BTreeMap<String, std::collections::BTreeSet<TypeDefId>>;

/// A cross-file `pub` / `pub(crate)` `use` RE-EXPORT index: the RE-EXPORTING
/// module-qualified id (`re_exporting_module::Name`, or the alias under
/// `use … as Name`) mapped to the FULL re-exported target path as written
/// (`["crate","resolver_core","shallow_file_state","BudgetExceededFailure"]`).
///
/// This is the PROVEN-re-export identity the conservative resolver needs to
/// resolve a qualified reference written through a re-export PATH whose module is
/// NOT a SUFFIX of (or equal to) the def's real home — including a too-short
/// ANCESTOR-shortened qualifier, which DIRECT-matches nothing and resolves ONLY
/// here: `semantic_query.rs` declares
/// `pub use crate::resolver_core::shallow_file_state::BudgetExceededFailure;`, so
/// a written `crate::semantic_query::BudgetExceededFailure` is the re-exporting
/// module's name for the def at `crate::resolver_core::shallow_file_state`. Only
/// `pub` / `pub(crate)` re-exports are recorded (a private `use`, or a narrow
/// `pub(self)`/`pub(in …)` re-export, does not create a crate-wide re-export path
/// other modules can write); a glob re-export (`pub use x::*`) names no specific
/// symbol and is skipped.
type ReExportIndex = BTreeMap<TypeDefId, std::collections::BTreeSet<Vec<String>>>;

/// Whether a `use` item's visibility makes it a GENUINE crate-wide re-export the
/// conservative resolver may trust as PROOF OF IDENTITY: ONLY `pub` and
/// `pub(crate)`. A `pub(self)` (path `self`) or a narrow `pub(in some::scope)`
/// re-export is visible to a SMALLER region than the crate, so it does NOT create
/// a re-export path an arbitrary other module can write — it MUST NOT feed the
/// re-export prover (treating it as crate-wide proof is fail-OPEN). A private
/// `use …` (Inherited) is likewise not a re-export.
///
/// `pub(crate)` is a `Restricted` whose path is EXACTLY `crate` (a single,
/// leading-colon-free segment whose sole ident is `crate`); `pub(self)`
/// (`Restricted` path `self`), `pub(super)`, and `pub(in …)` (any other
/// `Restricted` path) are narrower than crate-wide and are rejected.
fn use_is_reexport(vis: &syn::Visibility) -> bool {
    match vis {
        syn::Visibility::Public(_) => true,
        syn::Visibility::Restricted(r) => {
            r.path.leading_colon.is_none()
                && r.path.segments.len() == 1
                && r.path.segments[0].ident == "crate"
        }
        syn::Visibility::Inherited => false,
    }
}

/// Collect every file's `pub` / `pub(crate)` `use` RE-EXPORT items into a
/// [`ReExportIndex`], walking nested `use a::{b, c as d}` trees and `as`-renames.
/// Each recorded entry keys the re-exporting module-qualified id (the file's
/// module base + inline-`mod` stack, plus the re-exported name / alias) to the
/// full target path. A glob (`pub use x::*`) is skipped (it names no symbol). A
/// private (`use …`, no `pub`) import — and a NARROW `pub(self)` / `pub(in …)`
/// re-export (see [`use_is_reexport`]) — is skipped: it does not create a
/// crate-wide re-export path another module can write.
///
/// The source set is the union of [`type_def_source_files`] (the def homes) and
/// [`reexport_only_source_files`] (cross-crate re-export `mod.rs` files that
/// define no types but re-export the def homes up one module level), so a written
/// ancestor-shortened qualifier (`verter_semantic::analysis::AnalyzedBinding` for
/// the def at `verter_semantic::analysis::types::AnalyzedBinding`) is PROVED by a
/// genuine `pub use types::{…}` re-export through the now-load-bearing rail
/// instead of a bare ancestor-prefix match.
fn collect_reexport_index() -> ReExportIndex {
    struct V {
        module_stack: Vec<String>,
        idx: ReExportIndex,
    }
    fn walk_tree(
        tree: &syn::UseTree,
        prefix: &mut Vec<String>,
        module: &str,
        idx: &mut ReExportIndex,
    ) {
        match tree {
            syn::UseTree::Path(p) => {
                prefix.push(p.ident.to_string());
                walk_tree(&p.tree, prefix, module, idx);
                prefix.pop();
            }
            syn::UseTree::Name(n) => {
                let name = n.ident.to_string();
                let mut path = prefix.clone();
                path.push(name.clone());
                idx.entry(TypeDefId::new(module, &name))
                    .or_default()
                    .insert(path);
            }
            syn::UseTree::Rename(r) => {
                let mut path = prefix.clone();
                path.push(r.ident.to_string());
                idx.entry(TypeDefId::new(module, r.rename.to_string()))
                    .or_default()
                    .insert(path);
            }
            syn::UseTree::Group(g) => {
                for item in &g.items {
                    walk_tree(item, prefix, module, idx);
                }
            }
            syn::UseTree::Glob(_) => {}
        }
    }
    impl<'ast> syn::visit::Visit<'ast> for V {
        fn visit_item_use(&mut self, u: &'ast syn::ItemUse) {
            if !use_is_reexport(&u.vis) {
                return;
            }
            let module = self
                .module_stack
                .last()
                .cloned()
                .unwrap_or_else(|| "crate".to_string());
            let mut prefix = Vec::new();
            walk_tree(&u.tree, &mut prefix, &module, &mut self.idx);
        }
        fn visit_item_mod(&mut self, m: &'ast syn::ItemMod) {
            if mod_is_cfg_test(&m.attrs) {
                return;
            }
            let parent = self
                .module_stack
                .last()
                .cloned()
                .unwrap_or_else(|| "crate".to_string());
            self.module_stack.push(format!("{parent}::{}", m.ident));
            syn::visit::visit_item_mod(self, m);
            self.module_stack.pop();
        }
    }
    let mut idx: ReExportIndex = BTreeMap::new();
    let sources = type_def_source_files()
        .into_iter()
        .chain(reexport_only_source_files());
    for (module_base, src) in sources {
        if let Ok(file) = syn::parse_file(&src) {
            let mut v = V {
                module_stack: vec![module_base],
                idx: ReExportIndex::new(),
            };
            syn::visit::Visit::visit_file(&mut v, &file);
            for (k, paths) in v.idx {
                idx.entry(k).or_default().extend(paths);
            }
        }
    }
    idx
}

/// Cross-crate re-export `mod.rs` files that define NO types but `pub use`-export
/// the def homes one module level up — the genuine re-export targets a written
/// ancestor-shortened cross-crate qualifier resolves through. These feed ONLY the
/// [`ReExportIndex`] (via [`collect_reexport_index`], NOT the def-graph
/// [`collect_type_defs`] collector): each file declares zero
/// structs/enums/type-aliases, so it contributes re-export entries and no new
/// collision-index members or sink scopes. Returns `(module_base, src)` so a
/// re-export written `pub use types::{X}` in `verter_semantic/src/analysis/mod.rs`
/// keys `(verter_semantic::analysis, X)` → `["types", "X"]`, proving an
/// ancestor-shortened `verter_semantic::analysis::X` ref through the
/// now-load-bearing rail instead of a bare ancestor-prefix match.
fn reexport_only_source_files() -> Vec<(String, String)> {
    const REEXPORT_ONLY: &[(&str, &str)] = &[
        // `pub use types::{AnalyzedBinding, ExportSignature, Hash16, …}` — re-exports
        // the `analysis::types` def home up to `analysis`.
        (
            "../verter_semantic/src/analysis/mod.rs",
            "verter_semantic::analysis",
        ),
        // `pub use request::{ExpandedComponentTypes, …}` — re-exports the
        // `type_expand::request` def home up to `type_expand`.
        (
            "../verter_semantic/src/analysis/type_expand/mod.rs",
            "verter_semantic::analysis::type_expand",
        ),
        // `pub use prepared::{PreparedTypeDecl, PreparedValueDecl}` — re-exports the
        // `type_solver::prepared` def home up to `type_solver`.
        (
            "../verter_semantic/src/analysis/type_solver/mod.rs",
            "verter_semantic::analysis::type_solver",
        ),
    ];
    let mut out: Vec<(String, String)> = Vec::new();
    for (rel, module_base) in REEXPORT_ONLY {
        let path = crate_root().join(rel);
        if let Ok(src) = std::fs::read_to_string(&path) {
            out.push((module_base.to_string(), src));
        }
    }
    out
}

/// The visibility a `use` binding carries — the accessibility axis of the
/// intra-crate use-binding PROOF rail. A binding is a NAME a module brings into
/// its own scope; whether a DIFFERENT module may follow it depends on this:
///
///   - `Public` / `PubCrate` — usable from anywhere in the crate (a
///     `pub`/`pub(crate) use` re-publishes the name crate-wide).
///   - `Restricted` (`pub(super)` / `pub(self)` / `pub(in …)`) and `Inherited`
///     (a bare private `use`) — usable ONLY from the binding module itself or a
///     DESCENDANT module. A child `mod`'s `use super::X` legitimately follows
///     the parent's private binding; an unrelated module does not. The exact
///     `pub(in …)` ancestor region is approximated DOWN to descendant-only — a
///     conservative (fail-closed) under-approximation that can only REJECT a
///     legitimate binding, never ACCEPT a forged one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum UseVisibility {
    Public,
    PubCrate,
    Restricted,
    Inherited,
}

impl UseVisibility {
    /// Classify a `syn::Visibility` on a `use` item. `pub(crate)` is the
    /// `Restricted` whose path is EXACTLY `crate`; every other `Restricted`
    /// (`pub(self)`, `pub(super)`, `pub(in …)`) collapses to `Restricted`
    /// (descendant-only); a bare `use` is `Inherited`.
    fn classify(vis: &syn::Visibility) -> UseVisibility {
        match vis {
            syn::Visibility::Public(_) => UseVisibility::Public,
            syn::Visibility::Restricted(r) => {
                if r.path.leading_colon.is_none()
                    && r.path.segments.len() == 1
                    && r.path.segments[0].ident == "crate"
                {
                    UseVisibility::PubCrate
                } else {
                    UseVisibility::Restricted
                }
            }
            syn::Visibility::Inherited => UseVisibility::Inherited,
        }
    }

    /// Whether a binding declared in `binding_module` with this visibility is
    /// accessible from `referencing_module` (intra-crate). `Public`/`PubCrate`
    /// are crate-wide; `Restricted`/`Inherited` reach only the binding module or
    /// a descendant of it.
    fn accessible_from(self, binding_module: &str, referencing_module: &str) -> bool {
        match self {
            UseVisibility::Public | UseVisibility::PubCrate => true,
            UseVisibility::Restricted | UseVisibility::Inherited => {
                referencing_module == binding_module
                    || referencing_module.starts_with(&format!("{binding_module}::"))
            }
        }
    }
}

/// One target of a `use` binding: the FULL imported path as written (relative
/// `crate`/`self`/`super` segments preserved — the chain follower normalizes
/// them against the binding module) plus the binding's visibility.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct UseBindingTarget {
    target_path: Vec<String>,
    visibility: UseVisibility,
}

/// A MODULE-SCOPED, cross-file intra-crate `use`-binding PROOF index: the
/// `(binding_module, local_name)` pair a `use` item brings into scope, mapped to
/// its one-or-more targets (a name may be `use`-bound more than once across
/// `cfg` arms — all recorded; a genuine chain follows whichever target resolves).
///
/// This is the proof rail a reference in a CHILD module follows to its PARENT
/// module's (possibly private) `use`: the genuine `registry_decl` chain — a
/// `super::ResolvedTypeDeclaration` reference resolves because the parent
/// `component_meta_query_engine` carries a PRIVATE `use
/// super::declaration_metadata::ResolvedTypeDeclaration`, whose descendant-
/// visible binding the child legitimately follows to the real def home.
///
/// NARROW + FAIL-CLOSED (the rail does NOT approximate Rust name resolution
/// broadly): intra-crate only (an extern-crate-rooted target is NOT chased
/// through this rail — it resolves, if at all, through the def-graph seed homes +
/// the re-export rail); explicit NON-GLOB `use` items only (`use x::*` names no
/// symbol and is skipped); module/descendant visibility only; and the chain
/// follower is cycle-bounded. An unsupported / unclassifiable `use` form
/// contributes NO binding, so a reference relying on it stays `Unresolved`
/// (fail-closed — never a guess).
type UseBindingIndex = BTreeMap<(String, String), Vec<UseBindingTarget>>;

/// Collect every file's NON-GLOB `use` bindings (public AND private) into a
/// [`UseBindingIndex`], keyed `(binding_module, local_name)`. The source set is
/// the SAME real def-graph + re-export source set ([`type_def_source_files`] ∪
/// [`reexport_only_source_files`]) — so a binding in any module whose defs feed
/// the closure can be followed. The binding module is the file's module base
/// pushed with each inline `mod`. A glob `use x::*` is skipped (names no
/// symbol); the local name of a `use … as Alias` is the alias, else the final
/// segment.
fn collect_use_binding_index() -> UseBindingIndex {
    struct V {
        module_stack: Vec<String>,
        idx: UseBindingIndex,
    }
    fn walk_tree(
        tree: &syn::UseTree,
        prefix: &mut Vec<String>,
        module: &str,
        vis: UseVisibility,
        idx: &mut UseBindingIndex,
    ) {
        match tree {
            syn::UseTree::Path(p) => {
                prefix.push(p.ident.to_string());
                walk_tree(&p.tree, prefix, module, vis, idx);
                prefix.pop();
            }
            syn::UseTree::Name(n) => {
                let name = n.ident.to_string();
                let mut path = prefix.clone();
                path.push(name.clone());
                idx.entry((module.to_string(), name))
                    .or_default()
                    .push(UseBindingTarget {
                        target_path: path,
                        visibility: vis,
                    });
            }
            syn::UseTree::Rename(r) => {
                let mut path = prefix.clone();
                path.push(r.ident.to_string());
                idx.entry((module.to_string(), r.rename.to_string()))
                    .or_default()
                    .push(UseBindingTarget {
                        target_path: path,
                        visibility: vis,
                    });
            }
            syn::UseTree::Group(g) => {
                for item in &g.items {
                    walk_tree(item, prefix, module, vis, idx);
                }
            }
            // A glob (`use x::*`) names no specific symbol — fail-closed: it
            // contributes NO binding, so a reference relying on it stays
            // `Unresolved`.
            syn::UseTree::Glob(_) => {}
        }
    }
    impl<'ast> syn::visit::Visit<'ast> for V {
        fn visit_item_use(&mut self, u: &'ast syn::ItemUse) {
            let module = self
                .module_stack
                .last()
                .cloned()
                .unwrap_or_else(|| "crate".to_string());
            let vis = UseVisibility::classify(&u.vis);
            let mut prefix = Vec::new();
            walk_tree(&u.tree, &mut prefix, &module, vis, &mut self.idx);
        }
        fn visit_item_mod(&mut self, m: &'ast syn::ItemMod) {
            if mod_is_cfg_test(&m.attrs) {
                return; // test submodule — production-unreachable
            }
            let parent = self
                .module_stack
                .last()
                .cloned()
                .unwrap_or_else(|| "crate".to_string());
            self.module_stack.push(format!("{parent}::{}", m.ident));
            syn::visit::visit_item_mod(self, m);
            self.module_stack.pop();
        }
    }
    let mut idx: UseBindingIndex = BTreeMap::new();
    let sources = type_def_source_files()
        .into_iter()
        .chain(reexport_only_source_files());
    for (module_base, src) in sources {
        if let Ok(file) = syn::parse_file(&src) {
            let mut v = V {
                module_stack: vec![module_base],
                idx: UseBindingIndex::new(),
            };
            syn::visit::Visit::visit_file(&mut v, &file);
            for (k, targets) in v.idx {
                idx.entry(k).or_default().extend(targets);
            }
        }
    }
    idx
}

/// Resolve `crate` / `self` / `super` relative leading segments of a written
/// QUALIFIER against the referencing module, producing an ABSOLUTE
/// `crate::…`-rooted qualifier. A `crate::…` qualifier is returned unchanged; a
/// `self::…` qualifier is rebased onto `from_module`; each leading `super`
/// pops one segment off `from_module`. A qualifier whose leading segment is
/// none of these (a sibling-relative `route_demand::X`, or a cross-crate
/// `verter_type_expr::X`) is returned unchanged (the suffix matcher handles
/// those). Returns `None` the moment a `super` would pop the final / `crate`
/// segment — you cannot go ABOVE the crate root, so an over-deep `super` chain is
/// an un-rootable path and fails closed (popping below the root would leave a
/// bare `["X"]` that matches loosely).
fn normalize_relative_qualifier(qualifier: &[String], from_module: &str) -> Option<Vec<String>> {
    if qualifier.is_empty() {
        return Some(Vec::new());
    }
    match qualifier[0].as_str() {
        "crate" => Some(qualifier.to_vec()),
        "self" => {
            let mut out: Vec<String> = from_module.split("::").map(str::to_string).collect();
            out.extend(qualifier[1..].iter().cloned());
            Some(out)
        }
        "super" => {
            let mut base: Vec<String> = from_module.split("::").map(str::to_string).collect();
            let mut rest = qualifier;
            while rest.first().map(String::as_str) == Some("super") {
                // A `super` may not pop the final / `crate` segment — that would
                // escape ABOVE the crate root. Fail closed BEFORE popping the last
                // segment (one pop too late would leave a bare `["X"]`).
                if base.len() <= 1 {
                    return None;
                }
                base.pop();
                rest = &rest[1..];
            }
            base.extend(rest.iter().cloned());
            Some(base)
        }
        _ => Some(qualifier.to_vec()),
    }
}

/// Whether a single candidate def is a PROVEN target of the written qualified
/// path through a `pub` / `pub(crate)` RE-EXPORT: the written path (already
/// NORMALIZED to an absolute `crate::…`-rooted path by the caller, so a relative
/// `super::…` re-export reference keys the absolutely-indexed rail) names a
/// re-exporting module whose re-export of that name targets the candidate's real
/// home EXACTLY — a ROOTED-NORMALIZED EQUAL match (the normalized absolute target
/// module == `candidate.module`), an EXACT child-relative match
/// (`re_exporting_module::<bare-target-qualifier>` == `candidate.module`), OR a
/// TRANSITIVE re-export hop (the frontier chase). SUFFIX slack on the re-export
/// TARGET is fail-OPEN — a single-segment `pub use publication_authority::X` would
/// suffix-match ANY `…::publication_authority` home — so the target comparison is
/// EXACT, never suffix. (Suffix-or-equal stays correct for DISAMBIGUATING a
/// written DIRECT qualifier in [`module_qualifier_matches`]; this is ONLY the
/// re-export-prover's target-module comparison.) This is the proven-identity
/// branch the conservative resolver uses INSTEAD of a unique-name shortcut: a
/// fabricated `external::AdmittedPublishedMember` qualifier names no re-export and
/// so does NOT resolve.
fn qualified_path_is_proven_reexport_of(
    path: &[String],
    candidate: &TypeDefId,
    reexports: &ReExportIndex,
) -> bool {
    if path.len() < 2 {
        return false;
    }
    let name = match path.last() {
        Some(n) => n.clone(),
        None => return false,
    };
    let written_module = path[..path.len() - 1].join("::");
    // The ABSOLUTE candidate modules a re-export TARGET path's qualifier denotes,
    // relative to the re-exporting `module`. A `crate` / `self` / `super` leading
    // segment normalizes against `module`; a bare leading segment is AMBIGUOUS in
    // a `use` (an extern crate name OR a child `mod` of the re-exporting module),
    // so BOTH interpretations are returned (the absolute form AND the
    // child-relative `module::<qualifier>` form) — a re-export identity holds if
    // EITHER denotes the candidate's home.
    fn target_qualifier_modules(target_qualifier: &[String], module: &str) -> Vec<Vec<String>> {
        if target_qualifier.is_empty() {
            return vec![module.split("::").map(str::to_string).collect()];
        }
        match target_qualifier[0].as_str() {
            "crate" | "self" | "super" => normalize_relative_qualifier(target_qualifier, module)
                .into_iter()
                .collect(),
            _ => {
                let absolute = target_qualifier.to_vec();
                let mut child: Vec<String> = module.split("::").map(str::to_string).collect();
                child.extend(target_qualifier.iter().cloned());
                vec![absolute, child]
            }
        }
    }
    // Follow re-export hops from the written `(module, name)` id, bounded by a
    // seen-set to avoid a cycle, checking whether any hop's TARGET path denotes
    // the candidate's real home (an EXACT module match) with the same final name
    // (a re-export keeps the name unless renamed, and a rename is recorded under
    // the alias key so the name still matches the hop).
    let mut seen: std::collections::BTreeSet<TypeDefId> = std::collections::BTreeSet::new();
    let mut frontier: Vec<TypeDefId> = vec![TypeDefId::new(&written_module, &name)];
    while let Some(cur) = frontier.pop() {
        if !seen.insert(cur.clone()) {
            continue;
        }
        let Some(targets) = reexports.get(&cur) else {
            continue;
        };
        for target in targets {
            let Some(target_name) = target.last() else {
                continue;
            };
            let target_qualifier = &target[..target.len() - 1];
            for normalized in target_qualifier_modules(target_qualifier, &cur.module) {
                // A re-export target whose module is the candidate's real home
                // EXACTLY (never suffix slack) AND names the candidate is a proven
                // identity.
                if *target_name == candidate.name && normalized.join("::") == candidate.module {
                    return true;
                }
                // Otherwise chase the next re-export hop (`raise` re-exports from
                // `output_materialization`, which re-exports from `carrier`).
                frontier.push(TypeDefId::new(normalized.join("::"), target_name));
            }
        }
    }
    false
}

/// Whether a candidate module path (`::`-split into segments) is a DIRECT match
/// for the written qualifier segments — used ONLY to disambiguate a COLLIDING
/// name (the same bare name defined in two modules). A candidate matches when the
/// written qualifier is a SUFFIX of (or EQUAL to) the candidate module path
/// (`semantic_query::IndexSignature` / `crate::semantic_query::IndexSignature`
/// both pick `crate::semantic_query`). A genuine written qualifier is ALWAYS the
/// tail of the real module path; an arbitrary ANCESTOR PREFIX is NOT proof of
/// identity (a too-short `["crate"]` would prefix-match every `crate::…` module),
/// so the prefix is NOT accepted here — an ancestor-shortened / deliberately
/// re-exported reference resolves ONLY through
/// [`qualified_path_is_proven_reexport_of`] (the `ReExportIndex` rail), which
/// this suffix-or-equal contract makes genuinely LOAD-BEARING. This stays sound
/// for the `IndexSignature` crux — `crate::semantic_query` and `verter_type_expr`
/// are not a suffix of each other, so each qualifier still selects exactly one
/// home.
fn module_qualifier_matches(candidate_module: &str, qualifier: &[String]) -> bool {
    if qualifier.is_empty() {
        return true;
    }
    let mod_segs: Vec<&str> = candidate_module.split("::").collect();
    if qualifier.len() > mod_segs.len() {
        return false;
    }
    mod_segs[mod_segs.len() - qualifier.len()..]
        .iter()
        .zip(qualifier)
        .all(|(m, q)| *m == q.as_str())
}

/// Follow an intra-crate `use`-binding CHAIN from `(binding_module, name)` to a
/// concrete [`TypeDefId`], or `None` if no genuine accessible binding proves one.
/// This is the narrow proof rail a CHILD module's reference follows to its PARENT
/// module's (possibly private) `use`: the genuine `registry_decl` chain resolves
/// `super::ResolvedTypeDeclaration` because the parent `component_meta_query_engine`
/// carries a private `use super::declaration_metadata::ResolvedTypeDeclaration`,
/// whose descendant-visible binding the child legitimately follows to the real def
/// home.
///
/// Cycle-bounded by `seen` (a binding may legitimately point at another bound
/// name across modules; a cycle yields no proof, fail-closed). A binding is
/// followed ONLY when ACCESSIBLE from `referencing_module` (the original ref site)
/// — `pub`/`pub(crate)` crate-wide, a private/`pub(super)`/`pub(in …)` binding only
/// from the binding module or a descendant. The binding TARGET is normalized
/// against the binding module; an EXTERN-crate-rooted target (a non-`crate`-rooted
/// absolute path) is NOT chased through this intra-crate rail (it resolves, if at
/// all, through the def-graph seed homes + the re-export rail). Each chased target
/// is re-resolved as a fresh qualified reference (so a multi-hop chain, or a final
/// DIRECT/re-export match, all terminate at a real def).
fn resolve_use_binding_chain(
    binding_module: &str,
    name: &str,
    referencing_module: &str,
    name_to_ids: &NameDefIndex,
    reexports: &ReExportIndex,
    use_bindings: &UseBindingIndex,
    seen: &mut std::collections::BTreeSet<(String, String)>,
) -> Option<TypeDefId> {
    let key = (binding_module.to_string(), name.to_string());
    if !seen.insert(key.clone()) {
        return None; // cycle — fail-closed
    }
    let targets = use_bindings.get(&key)?;
    for target in targets {
        if !target
            .visibility
            .accessible_from(binding_module, referencing_module)
        {
            continue;
        }
        let Some(target_name) = target.target_path.last() else {
            continue;
        };
        let qualifier = &target.target_path[..target.target_path.len() - 1];
        // Normalize the target qualifier against the BINDING module (a `super` /
        // `self` re-export references its own module). Only a `crate`-rooted
        // absolute result is an intra-crate target this rail chases; an
        // extern-crate-rooted (or un-rootable) target is not.
        let Some(abs_qualifier) = normalize_relative_qualifier(qualifier, binding_module) else {
            continue;
        };
        if abs_qualifier.first().map(String::as_str) != Some("crate") {
            continue; // not an intra-crate target — not chased through this rail
        }
        let mut abs_path = abs_qualifier;
        abs_path.push(target_name.clone());
        let resolved = resolve_type_ref_seen(
            &abs_path,
            referencing_module,
            &UseIndex::default(),
            name_to_ids,
            reexports,
            use_bindings,
            seen,
        );
        if let Some(id) = resolved.resolved() {
            return Some(id.clone());
        }
    }
    None
}

/// THE conservative, FAIL-CLOSED resolver: resolve one written reference path to
/// a single concrete [`TypeDefId`], or leave it `Unresolved`. This documents the
/// ACTUAL behavior of the 80/20 fail-closed classifier (see the section header),
/// NOT a complete-proof claim: it resolves the common in-tree shapes
/// (own-module-def, rooted `crate`/`self`/`super` direct paths, exact
/// `pub`/`pub(crate)` re-exports, ordinary file imports, the audited
/// `registry_decl` private `use`-binding chain) and unsupported shapes generally
/// remain `Unresolved` — but the accepted residuals below MAY OVER-RESOLVE (they
/// are not a complete fail-closed guarantee). Those residual shapes — where the
/// classifier may resolve WITHOUT proof — are called out per-arm below and
/// tracked in `docs/arch/output-projector-residual-guard-debt.md`.
///
///   - A FULLY-QUALIFIED path (≥2 segments): the final segment is the name. A
///     candidate is matched by ONE of:
///     (1) a DIRECT match — its real module is a SUFFIX of (or EQUAL to) the
///     qualifier — where a relative `crate` / `self` / `super` qualifier is first
///     rebased onto the referencing module (a `super` can never escape above the
///     crate root), and an UNROOTED first segment that the referencing file's
///     `use`-index SHADOWS is re-resolved through the shadow binding (so a
///     `use crate::other as publication_authority` cannot bless
///     `publication_authority::X`); a too-short ANCESTOR prefix is never a direct
///     match. RESIDUAL (accepted, debt): an UNROOTED UNSHADOWED qualifier is
///     trusted on its RAW SUFFIX — `publication_authority::AdmittedPublishedMember`
///     written from a file with no shadowing `use` raw-suffix matches the unique
///     collected token. This is not proof; it is accepted because the token is
///     uniquely named (the match lands on the single genuine def) and the
///     compiler-enforced sealed-token boundary is the production guarantee.
///     (2) a `pub` / `pub(crate)` RE-EXPORT of that candidate whose TARGET module
///     is the candidate's real home EXACTLY (an ancestor-shortened or
///     relative-qualified re-export reference resolves through THIS rail, keyed by
///     the NORMALIZED absolute written path:
///     `crate::semantic_query::BudgetExceededFailure` through the `semantic_query`
///     re-export of the `shallow_file_state` def). (3) a proven intra-crate
///     `use`-binding CHAIN at the normalized qualifier module (the genuine
///     `super::ResolvedTypeDeclaration` chain through the parent module's private
///     `use`). A COLLIDING name (`IndexSignature` — `crate::semantic_query` vs
///     `verter_type_expr`) is disambiguated the same way into DISTINCT IDs. A
///     qualifier that matches NONE ⇒ `Unresolved` (fail-closed); zero collected
///     defs ⇒ `Unresolved` (an unread external type, e.g.
///     `std::collections::BTreeMap`, is benign and classified by its final segment
///     downstream).
///   - An UNQUALIFIED path (one segment): the referencing file's own module def
///     first; then, if a `use` import in that file UNIQUELY claims the name
///     (`uses.unique_path` returns one target), that import's TARGET is resolved
///     and returned AS-IS — if the import target does NOT resolve, the name stays
///     `Unresolved` IMMEDIATELY (`use external::X as AdmittedPublishedMember` over
///     a unique `AdmittedPublishedMember` does not bless the token via this arm);
///     else a parent module's accessible `use`-binding chain may resolve it; else,
///     exactly-one collected def with that name. RESIDUAL (accepted debt, class B
///     — see the section header's root-cause classes): the `candidates.len() == 1`
///     global-uniqueness fallback is reached whenever a unique PROVEN import target
///     was not found — for a no-import bare name, an AMBIGUOUS MULTI-import
///     (`unique_path` is `None` for >1 target), AND a unique SINGLE-SEGMENT
///     self-import (`use Foo;`, where the recursion guard skips the import-claim
///     arm because the import path IS the bare name) — so the "no-import" framing
///     alone is over-stated. A parallel residual lives on the use-binding CHAIN
///     (`resolve_use_binding_chain` returns the FIRST accessible target that
///     resolves, not a single proven one). The global-uniqueness / first-binding
///     resolve is not proof; it is accepted for the same reason as the qualified
///     residual above (unique-named tokens + sound compiler primary). More than
///     one collected def and no unique resolution path ⇒ `Unresolved`.
fn resolve_type_ref(
    path: &[String],
    from_module: &str,
    uses: &UseIndex,
    name_to_ids: &NameDefIndex,
    reexports: &ReExportIndex,
    use_bindings: &UseBindingIndex,
) -> TypeRef {
    let mut seen = std::collections::BTreeSet::new();
    resolve_type_ref_seen(
        path,
        from_module,
        uses,
        name_to_ids,
        reexports,
        use_bindings,
        &mut seen,
    )
}

/// The cycle-bounded core of [`resolve_type_ref`]. `seen` bounds the intra-crate
/// `use`-binding chain (shared across the qualified-arm chain + the import-target
/// recursion + the unrooted-shadow rebind, so a cyclic chain across any of those
/// terminates fail-closed).
fn resolve_type_ref_seen(
    path: &[String],
    from_module: &str,
    uses: &UseIndex,
    name_to_ids: &NameDefIndex,
    reexports: &ReExportIndex,
    use_bindings: &UseBindingIndex,
    seen: &mut std::collections::BTreeSet<(String, String)>,
) -> TypeRef {
    let final_segment = path.last().cloned().unwrap_or_default();
    let unresolved = || TypeRef::Unresolved {
        from_module: from_module.to_string(),
        path: path.to_vec(),
        final_segment: final_segment.clone(),
    };
    let empty = std::collections::BTreeSet::new();
    let candidates = name_to_ids.get(&final_segment).unwrap_or(&empty);

    if path.len() >= 2 {
        // Qualified. The written qualifier must be PROVED to name the candidate by
        // a DIRECT suffix-or-equal match, an EXACT-target `pub`/`pub(crate)`
        // re-export, or an intra-crate `use`-binding chain. A too-short ANCESTOR
        // qualifier is NOT a direct match; uniqueness ALONE never resolves a
        // qualified path (a fabricated `external::Foo` over a unique `Foo` stays
        // `Unresolved`) — the load-bearing fail-closed property.
        let qualifier = &path[..path.len() - 1];
        let normalized = normalize_relative_qualifier(qualifier, from_module);
        // The ABSOLUTE written path the re-export prover keys the index with: a
        // relative-qualified re-export reference (`super::output_materialization::
        // X` written in a `pub(crate) use`) must be rebased to its absolute module
        // BEFORE the prover looks it up, or it misses the absolutely-keyed index.
        // When the qualifier is un-rootable (`super` above the crate root) there is
        // no absolute path and the prover is not consulted (fail-closed).
        let normalized_path: Option<Vec<String>> = normalized.as_ref().map(|q| {
            let mut p = q.clone();
            p.push(final_segment.clone());
            p
        });
        // STEP-4 unrooted-shadow proof: an UNROOTED first segment the referencing
        // file `use`-SHADOWS is NOT trusted on its raw suffix (the qualifier is a
        // `use`-alias for a DIFFERENT module). Re-resolve through the shadow
        // binding: the bound path + the remaining qualifier + the name, as a fresh
        // qualified reference. A `use crate::other as publication_authority` then
        // `publication_authority::X` resolves (only) to what `crate::other::X`
        // proves — never the raw-suffix-matched safe token.
        let unrooted = !matches!(qualifier[0].as_str(), "crate" | "self" | "super");
        let unrooted_shadowed = unrooted && uses.imports.contains_key(&qualifier[0]);
        let via_shadow: Option<TypeDefId> = if unrooted_shadowed {
            uses.unique_path(&qualifier[0]).and_then(|bound| {
                let mut rebound: Vec<String> = bound.clone();
                rebound.extend_from_slice(&qualifier[1..]);
                rebound.push(final_segment.clone());
                resolve_type_ref_seen(
                    &rebound,
                    from_module,
                    &UseIndex::default(),
                    name_to_ids,
                    reexports,
                    use_bindings,
                    seen,
                )
                .resolved()
                .cloned()
            })
        } else {
            None
        };
        // STEP-1/3 intra-crate use-binding chain at the normalized qualifier module
        // (the genuine `super::ResolvedTypeDeclaration` chain).
        let via_binding: Option<TypeDefId> = normalized.as_deref().and_then(|q| {
            let module = q.join("::");
            resolve_use_binding_chain(
                &module,
                &final_segment,
                from_module,
                name_to_ids,
                reexports,
                use_bindings,
                seen,
            )
        });
        let candidate_matches = |id: &TypeDefId| -> bool {
            // (1) DIRECT suffix-or-equal match against the (rooted-normalized or
            // raw) qualifier — DISABLED when the qualifier's UNROOTED first segment
            // is a `use`-shadowed alias (then the raw suffix is not trusted; the
            // qualifier is proven only through `via_shadow`). For a ROOTED qualifier
            // `normalized` is the absolute rebased path and `qualifier` the raw
            // leading-`crate`/`self`/`super` form; for an UNROOTED unshadowed
            // qualifier both equal the raw written segments.
            let direct = !unrooted_shadowed
                && (normalized
                    .as_deref()
                    .is_some_and(|q| module_qualifier_matches(&id.module, q))
                    || module_qualifier_matches(&id.module, qualifier));
            direct
                // (2) EXACT-target proven re-export.
                || normalized_path
                    .as_deref()
                    .is_some_and(|p| qualified_path_is_proven_reexport_of(p, id, reexports))
                // (3) proven intra-crate use-binding chain.
                || via_binding.as_ref() == Some(id)
                // STEP-4 shadow-rebind proof.
                || via_shadow.as_ref() == Some(id)
        };
        let matches: Vec<&TypeDefId> = candidates
            .iter()
            .filter(|id| candidate_matches(id))
            .collect();
        return match matches.as_slice() {
            [one] => TypeRef::Resolved((*one).clone()),
            _ => unresolved(),
        };
    }

    // Unqualified — the 80/20 classifier's arms (a)-(d). Common shapes (a)-(c)
    // resolve by genuine proof; (d) is the accepted-residual global-uniqueness
    // fallback (tracked in docs/arch/output-projector-residual-guard-debt.md).
    // (a) own module def.
    let own = TypeDefId::new(from_module, &final_segment);
    if candidates.contains(&own) {
        return TypeRef::Resolved(own);
    }
    // (b) a `use` import in this file UNIQUELY claims the name (`unique_path`
    // returns one target) — resolve its TARGET by proof and return AS-IS. If the
    // import target does NOT resolve, the name stays `Unresolved` IMMEDIATELY via
    // this arm. RESIDUAL (class B, accepted debt): this arm is SKIPPED — falling
    // through to the (d) global-uniqueness fallback — when `unique_path` is `None`
    // for an AMBIGUOUS MULTI-import (two `use`s claim the name) AND when the import
    // is a unique SINGLE-SEGMENT self-import (`use Foo;`, where the recursion guard
    // below is false because the import path IS the bare name); both then resolve
    // by collected uniqueness, not proof.
    if let Some(import_path) = uses.unique_path(&final_segment) {
        // The import names a qualified path (its own final segment may differ
        // under a rename); resolve THAT path. Guard against a self-referential
        // single-segment import that would re-enter this branch endlessly (a
        // `use Foo;` falls through to (d) — disclosed as a class-B residual).
        if import_path.len() >= 2 || import_path.last() != Some(&final_segment) {
            return resolve_type_ref_seen(
                import_path,
                from_module,
                &UseIndex::default(),
                name_to_ids,
                reexports,
                use_bindings,
                seen,
            );
        }
    }
    // (c) a parent module's accessible intra-crate `use`-binding chain proves the
    // name (a bare reference whose binding lives in an ancestor module's private
    // `use`, not a file-level import in THIS file).
    if let Some(id) = resolve_use_binding_chain(
        from_module,
        &final_segment,
        from_module,
        name_to_ids,
        reexports,
        use_bindings,
        seen,
    ) {
        return TypeRef::Resolved(id);
    }
    // (d) exactly-one collected def with that name — the global-uniqueness
    // fallback. Reached when no UNIQUE import path was returned at (b): the
    // benign case (no import claimed the name, a single collected def genuinely
    // names it) PLUS the accepted RESIDUAL where an AMBIGUOUS multi-import left
    // `unique_path` `None`. This is global uniqueness, not proof; accepted
    // because the sanctioned tokens are uniquely named + the sealed-token
    // compiler boundary is the production guarantee (debt-tracked). More than
    // one ⇒ AMBIGUOUS, fail-closed.
    match candidates.len() {
        1 => TypeRef::Resolved(candidates.iter().next().unwrap().clone()),
        _ => unresolved(),
    }
}

/// Build the qualified type-def collection over the read roots: a TWO-PASS
/// resolve. Pass 1 collects every definition's RAW reference paths under its
/// module-qualified [`TypeDefId`] plus the file's `use`-index, and builds the
/// `name -> {TypeDefId}` collision index. Pass 2 resolves every raw reference to
/// a concrete `TypeDefId` (or `Unresolved`) via the conservative
/// [`resolve_type_ref`]. Returns the resolved def-graph plus the collision index.
fn collect_type_defs() -> (BTreeMap<TypeDefId, TypeDefRefs>, NameDefIndex) {
    // Pass 1: per-file raw collection.
    struct FileDefs {
        raw_defs: BTreeMap<TypeDefId, Vec<Vec<String>>>,
        uses: UseIndex,
    }
    let mut per_file: Vec<FileDefs> = Vec::new();
    let mut name_to_ids: NameDefIndex = BTreeMap::new();
    let mut alias_targets: BTreeMap<TypeDefId, Vec<String>> = BTreeMap::new();
    // The cross-file `pub` / `pub(crate)` RE-EXPORT index + the module-scoped
    // intra-crate `use`-binding PROOF index — the proven-identity rails the
    // conservative qualified-path resolver uses INSTEAD of a unique-name shortcut.
    let reexports = collect_reexport_index();
    let use_bindings = collect_use_binding_index();
    for (module_base, src) in type_def_source_files() {
        if let Ok(file) = syn::parse_file(&src) {
            let mut collector = TypeDefCollector::with_module_base(module_base);
            syn::visit::Visit::visit_file(&mut collector, &file);
            let uses = collect_use_index(&file);
            for id in collector.raw_defs.keys() {
                name_to_ids
                    .entry(id.name.clone())
                    .or_default()
                    .insert(id.clone());
            }
            alias_targets.extend(collector.alias_targets);
            per_file.push(FileDefs {
                raw_defs: collector.raw_defs,
                uses,
            });
        }
    }
    // Collapse same-name re-export aliases: a `type X = <path>` alias whose RHS
    // resolves (by name + qualifier) to ANOTHER same-name def is NOT a distinct
    // cross-module type — it is a re-export of the real def. Drop the alias id
    // from the collision index so the name stays UNAMBIGUOUS for resolution
    // (`pub type ResolvedTypeDeclaration = crate::resolver_core::
    // ResolvedTypeDeclaration;` collapses onto the struct). A genuine collision
    // between two DISTINCT types (the `IndexSignature` crux) is untouched —
    // neither is a same-name alias of the other.
    {
        let resolve_index = name_index_with_seed_ids(&name_to_ids);
        let collapse: Vec<TypeDefId> = alias_targets
            .iter()
            .filter_map(|(alias_id, rhs_path)| {
                if rhs_path.last().map(|s| s.as_str()) != Some(alias_id.name.as_str()) {
                    return None; // RHS names a DIFFERENT type — not a self re-export
                }
                let resolved = resolve_type_ref(
                    rhs_path,
                    &alias_id.module,
                    &UseIndex::default(),
                    &resolve_index,
                    &reexports,
                    &use_bindings,
                );
                match resolved.resolved() {
                    Some(target) if target != alias_id => Some(alias_id.clone()),
                    _ => None,
                }
            })
            .collect();
        for alias_id in collapse {
            if let Some(ids) = name_to_ids.get_mut(&alias_id.name) {
                ids.remove(&alias_id);
            }
        }
    }
    // Pass 2: resolve each raw reference against the file's imports + the global
    // collision index, FOLDING IN the canonical seed ids so a reference naming a
    // seed resolves to it even when the seed's home file is unread
    // (cross-crate robustness). The RETURNED collision index stays
    // COLLECTED-only so the safe-input anti-vacuity check sees real defs, never a
    // synthetic seed id.
    let resolve_index = name_index_with_seed_ids(&name_to_ids);
    let mut defs: BTreeMap<TypeDefId, TypeDefRefs> = BTreeMap::new();
    for file in &per_file {
        for (id, raw_refs) in &file.raw_defs {
            let entry = defs.entry(id.clone()).or_default();
            for path in raw_refs {
                let tref = resolve_type_ref(
                    path,
                    &id.module,
                    &file.uses,
                    &resolve_index,
                    &reexports,
                    &use_bindings,
                );
                entry.refs.insert(tref);
            }
        }
    }
    (defs, name_to_ids)
}

/// The CANONICAL test module for a fixture def NAME: if the name is a seed /
/// safe-input / chain token with EXACTLY ONE qualified home across all the
/// `(module, name)` const arrays, return that home (so a synthetic
/// `AdmittedPublishedMember` def lands at the real `publication_authority`
/// module the closure exclusion keys on); otherwise a generic synthetic module.
/// An AMBIGUOUS name (`IndexSignature`, `ResolvedMacroPayload`) returns the
/// generic module — the discriminating self-tests that need those collisions
/// build their fixtures DIRECTLY with explicit `TypeDefId`s.
fn canonical_test_module(name: &str) -> String {
    let mut homes: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    for (m, n) in OUTPUT_AUTHORITY_SEEDS
        .iter()
        .chain(INPUT_AUTHORITY_SEEDS.iter())
        .chain(POLICY_ADMITTED_SAFE_INPUTS.iter())
        .chain(SEALED_CONSTRUCTION_CHAIN_STRUCTS.iter())
    {
        if *n == name {
            homes.insert(*m);
        }
    }
    if homes.len() == 1 {
        homes.into_iter().next().unwrap().to_string()
    } else {
        "crate::test_synthetic".to_string()
    }
}

/// Build a SYNTHETIC qualified def-graph from concise bare-name entries (the
/// self-test fixture bridge): each `(name, &[ref-name])` becomes a
/// `TypeDefId { canonical_test_module(name), name }` whose refs are resolved
/// against the synthetic name index + the seed-merged index by the SAME
/// conservative resolver the real path uses. A ref naming a seed resolves to the
/// seed id; a ref naming another fixture def resolves to it; an ambiguous /
/// unknown ref stays `Unresolved`. Returns the def-graph PLUS the fixture name
/// index, so a companion [`synthetic_sig`] resolves sink-fn refs against the
/// same fixture defs.
fn synthetic_defs(entries: &[(&str, &[&str])]) -> (BTreeMap<TypeDefId, TypeDefRefs>, NameDefIndex) {
    let mut name_to_ids: NameDefIndex = BTreeMap::new();
    let mut keys: Vec<TypeDefId> = Vec::new();
    for (name, _) in entries {
        let id = TypeDefId::new(canonical_test_module(name), *name);
        name_to_ids
            .entry(id.name.clone())
            .or_default()
            .insert(id.clone());
        keys.push(id);
    }
    let resolve_index = name_index_with_seed_ids(&name_to_ids);
    let mut defs: BTreeMap<TypeDefId, TypeDefRefs> = BTreeMap::new();
    for ((_, refs), id) in entries.iter().zip(keys) {
        let entry = defs.entry(id.clone()).or_default();
        for r in *refs {
            let tref = resolve_type_ref(
                std::slice::from_ref(&r.to_string()),
                &id.module,
                &UseIndex::default(),
                &resolve_index,
                &ReExportIndex::default(),
                &UseBindingIndex::default(),
            );
            entry.refs.insert(tref);
        }
    }
    (defs, name_to_ids)
}

/// Whether a `TypeDefId` set contains ANY id with the given bare NAME (a
/// self-test convenience: the fixtures use unique names, so "any id of this
/// name" is the right membership test there).
fn set_contains_name(set: &std::collections::BTreeSet<TypeDefId>, name: &str) -> bool {
    set.iter().any(|id| id.name == name)
}

/// The synthetic `TypeDefId` a fixture name resolves to (the same id
/// [`synthetic_defs`] / [`synthetic_sig`] mint), so a self-test can insert a
/// matching bearing/forgeable id into a closure set (e.g. an aliased output type
/// `F` modelled as bearing).
fn synthetic_id(name: &str) -> TypeDefId {
    TypeDefId::new(canonical_test_module(name), name)
}

/// A `TypeRef::Resolved` to the given qualified `(module, name)` — for the
/// discriminating self-tests that build their def-graphs DIRECTLY with explicit
/// module-qualified identity (the `IndexSignature` / `ResolvedMacroPayload`
/// collision cases that the bare-name `synthetic_defs` cannot model).
fn resolved_ref(module: &str, name: &str) -> TypeRef {
    TypeRef::Resolved(TypeDefId::new(module, name))
}

/// Resolve a single [`TypeDefCollector`]'s RAW defs into a qualified def-graph +
/// name index (the per-file analogue of [`collect_type_defs`] pass 2, with no
/// `use`-imports). A self-test driving the REAL collector on source uses this to
/// run the closures over genuinely-resolved refs.
fn defs_from_collector(
    collector: &TypeDefCollector,
) -> (BTreeMap<TypeDefId, TypeDefRefs>, NameDefIndex) {
    let mut name_to_ids: NameDefIndex = BTreeMap::new();
    for id in collector.raw_defs.keys() {
        name_to_ids
            .entry(id.name.clone())
            .or_default()
            .insert(id.clone());
    }
    let resolve_index = name_index_with_seed_ids(&name_to_ids);
    let mut defs: BTreeMap<TypeDefId, TypeDefRefs> = BTreeMap::new();
    for (id, raw_refs) in &collector.raw_defs {
        let entry = defs.entry(id.clone()).or_default();
        for path in raw_refs {
            entry.refs.insert(resolve_type_ref(
                path,
                &id.module,
                &UseIndex::default(),
                &resolve_index,
                &ReExportIndex::default(),
                &UseBindingIndex::default(),
            ));
        }
    }
    (defs, name_to_ids)
}

/// A synthetic `SinkFnSig` from bare in/out/mut-out names, resolved against the
/// fixture name index (`fixture` — the `name_to_ids` [`synthetic_defs`] returns,
/// or an empty map) PLUS the seed-merged index — the resolver-driven analogue of
/// the old bare-ident `SinkFnSig` builder. A name that is a seed / token / fixture
/// def resolves to its id; an UNKNOWN name stays `Unresolved` (so the fail-closed
/// completeness check genuinely catches an unread type). A primitive / generic
/// param stays unresolved and is skipped by the PascalCase gate.
///
/// Returns a non-test, non-module-private sig by default; the rare callers that
/// need those flags chain [`SinkFnSig::marked_test_gated`] / [`SinkFnSig::marked_module_private`].
fn synthetic_sig(
    fixture: &NameDefIndex,
    module: &str,
    name: &str,
    inputs: &[&str],
    outputs: &[&str],
    mut_out: &[&str],
) -> SinkFnSig {
    let resolve_index = name_index_with_seed_ids(fixture);
    let resolve_set = |names: &[&str]| -> std::collections::BTreeSet<TypeRef> {
        names
            .iter()
            .map(|r| {
                resolve_type_ref(
                    std::slice::from_ref(&r.to_string()),
                    module,
                    &UseIndex::default(),
                    &resolve_index,
                    &ReExportIndex::default(),
                    &UseBindingIndex::default(),
                )
            })
            .collect()
    };
    SinkFnSig {
        module_path: module.to_string(),
        name: name.to_string(),
        input_idents: resolve_set(inputs),
        output_idents: resolve_set(outputs),
        mut_outparam_idents: resolve_set(mut_out),
        test_gated: false,
        module_private: false,
    }
}

impl SinkFnSig {
    /// Builder: mark this synthetic sig `#[cfg(test)]`-gated (production-absent).
    fn marked_test_gated(mut self) -> Self {
        self.test_gated = true;
        self
    }

    /// Builder: mark this synthetic sig module-private (unreachable cross-sink).
    fn marked_module_private(mut self) -> Self {
        self.module_private = true;
        self
    }
}

/// Compute the set of all `TypeDefId`s that are TypeExpr-bearing by STRUCTURAL
/// FIELD-CLOSURE: seed with the output-authority seed IDs, then iterate to a
/// fixed point — a def is added when any of its RESOLVED outgoing refs is
/// already in the set. Following `Vec<_>`/`Option<_>`/`Arc<_>`/`Box<_>`/tuple is
/// automatic: `type_segment_refs` lists each nested named type as a separate
/// reference, so a field `Vec<TypeExpr>` carries a ref that resolves to the
/// `TypeExpr` seed id. An `Unresolved` ref never propagates membership (it is a
/// conservative non-edge); the boundary checks surface a dangerous unresolved
/// ref fail-closed.
fn typeexpr_bearing_closure(
    defs: &BTreeMap<TypeDefId, TypeDefRefs>,
) -> std::collections::BTreeSet<TypeDefId> {
    let mut bearing: std::collections::BTreeSet<TypeDefId> = output_authority_seed_ids();
    loop {
        let mut added = false;
        for (id, def) in defs {
            if bearing.contains(id) {
                continue;
            }
            if def
                .refs
                .iter()
                .any(|r| r.resolved().is_some_and(|t| bearing.contains(t)))
            {
                bearing.insert(id.clone());
                added = true;
            }
        }
        if !added {
            break;
        }
    }
    bearing
}

/// One collected production fn / method / trait-item in a scanned sink scope,
/// with the structural facts the policy needs. Input / output / out-param types
/// are carried as resolved [`TypeRef`]s (module-qualified identity), so the
/// boundary check classifies by genuine `(module, name)` identity and an
/// `Unresolved` ref at a bearing boundary is caught fail-closed.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct SinkFnSig {
    /// `::`-joined module path of the file the fn lives in.
    module_path: String,
    /// The fn name.
    name: String,
    /// Type refs referenced by ANY param type (`&mut` out-params included).
    input_idents: std::collections::BTreeSet<TypeRef>,
    /// Type refs referenced by the return type.
    output_idents: std::collections::BTreeSet<TypeRef>,
    /// Type refs referenced by `&mut` out-params specifically (a mutated DTO
    /// out-param is an OUTPUT channel even when the return type is `()`).
    mut_outparam_idents: std::collections::BTreeSet<TypeRef>,
    /// Whether the fn is `#[cfg(test)]`-gated (excluded — production-only scan).
    test_gated: bool,
    /// Whether the fn is provably reachable ONLY from within its own module: a
    /// free fn / INHERENT-impl method whose visibility is `Inherited` (a bare
    /// `fn`) or `pub(self)`. A module-private fn is NOT a cross-sink boundary —
    /// the cross-sink scan walks the whole subtree, but Rust visibility keeps a
    /// module-private fn unreachable from a sibling module, so it cannot pair a
    /// forgeable input with a TypeExpr output ACROSS a sink boundary. A
    /// trait-associated method (a trait-impl method or a trait-method
    /// declaration) is NEVER module-private here: its reachability rides the
    /// trait's visibility, not its own (always-`Inherited`) syntactic vis, so it
    /// stays a boundary subject to the allowlist.
    module_private: bool,
}

/// Derive the `::`-joined module path for a crate-relative `src/...rs` file,
/// e.g. `src/meta_resolve/projectors/props.rs` ->
/// `crate::meta_resolve::projectors::props`, `src/foo/mod.rs` -> `crate::foo`,
/// `src/lib.rs` -> `crate`.
fn module_path_for_rel(rel: &str) -> String {
    let stripped = rel.strip_prefix("src/").unwrap_or(rel);
    let stripped = stripped.strip_suffix(".rs").unwrap_or(stripped);
    let mut segs: Vec<&str> = stripped.split('/').collect();
    if segs.last() == Some(&"mod") || segs.last() == Some(&"lib") {
        segs.pop();
    }
    let mut path = String::from("crate");
    for seg in segs {
        path.push_str("::");
        path.push_str(seg);
    }
    path
}

/// `syn` visitor collecting every free fn / inherent-impl method / trait-method
/// / trait-impl method in a file, recording its param + return type idents.
/// `#[cfg(test)]` inline modules are skipped (production-only scan); a
/// `#[cfg(test)]` fn is recorded with `test_gated = true` so the policy can
/// drop it.
///
/// GAP-7: a module-path STACK tracks the enclosing inline `mod`s, so a fn in an
/// inline `mod inner {}` is recorded under `<file>::inner` — NOT the file's
/// module path. Without the stack, an inline-submodule fn would be recorded
/// under the FILE path, so a `(file, fn)` allowlist entry would wrongly match a
/// SAME-named fn in a different inline submodule (the allowlist would be too
/// broad). The stack makes the recorded `module_path` precise per inline mod.
struct SinkFnCollector<'a> {
    /// The module-path stack: the file's base module path, pushed with each
    /// non-test inline `mod ident {}` entered.
    module_stack: Vec<String>,
    sigs: Vec<SinkFnSig>,
    /// Set while visiting the items of an `impl Trait for Type` block: a
    /// trait-impl method has no syntactic visibility (always `Inherited`) yet is
    /// reachable through the trait, so it must NOT be classified module-private.
    in_trait_impl: bool,
    /// The file's `use`-import index (for resolving an aliased / imported param
    /// or return type to its qualified `TypeDefId`).
    uses: UseIndex,
    /// The global `name -> {TypeDefId}` collision index (seed-merged) the
    /// resolver consults to qualify each referenced type.
    name_to_ids: &'a NameDefIndex,
    /// The global `pub` / `pub(crate)` re-export index so a param/return type
    /// written through a re-export path resolves to its real def.
    reexports: &'a ReExportIndex,
    /// The global module-scoped intra-crate `use`-binding PROOF index so a
    /// param/return type written through a private-`use` chain (a `super::X`
    /// reference bound by a parent module's private `use`) resolves to its real
    /// def.
    use_bindings: &'a UseBindingIndex,
}

impl SinkFnCollector<'_> {
    fn current_module(&self) -> String {
        self.module_stack
            .last()
            .cloned()
            .unwrap_or_else(|| "crate".to_string())
    }
}

/// Whether a fn/method visibility confines reachability to its OWN module: a
/// bare `fn` (`Inherited`) or `pub(self)`. `pub` / `pub(crate)` / `pub(super)` /
/// `pub(in ...)` all reach beyond the module, so they are NOT module-private.
fn visibility_is_module_private(vis: &syn::Visibility) -> bool {
    match vis {
        syn::Visibility::Inherited => true,
        syn::Visibility::Restricted(r) => {
            // `pub(self)` — the only `pub(in ...)` form that stays module-local.
            r.path.is_ident("self")
        }
        syn::Visibility::Public(_) => false,
    }
}

impl SinkFnCollector<'_> {
    /// Resolve every named type a `syn::Type` references to a [`TypeRef`] against
    /// the current module + the file's imports + the global collision index, so
    /// a `crate::semantic_query::SemanticNodeId` param qualifies to that
    /// `TypeDefId` (and an ambiguous bare `IndexSignature` stays `Unresolved`).
    fn resolve_ty(&self, ty: &syn::Type) -> Vec<TypeRef> {
        let module = self.current_module();
        type_segment_refs(ty)
            .iter()
            .map(|path| {
                resolve_type_ref(
                    path,
                    &module,
                    &self.uses,
                    self.name_to_ids,
                    self.reexports,
                    self.use_bindings,
                )
            })
            .collect()
    }

    fn record(
        &mut self,
        name: String,
        sig: &syn::Signature,
        attrs: &[syn::Attribute],
        module_private: bool,
    ) {
        let mut input_idents = std::collections::BTreeSet::new();
        let mut mut_outparam_idents = std::collections::BTreeSet::new();
        for input in &sig.inputs {
            if let syn::FnArg::Typed(pat) = input {
                // MODULE-QUALIFIED identity: a `crate::semantic_query::
                // SemanticNodeId` param resolves to that `TypeDefId`, so the
                // forgeable-authority membership check sees genuine identity.
                let refs = self.resolve_ty(&pat.ty);
                // A `&mut T` param is an OUT channel; record its refs separately.
                if let syn::Type::Reference(r) = pat.ty.as_ref() {
                    if r.mutability.is_some() {
                        for tref in &refs {
                            mut_outparam_idents.insert(tref.clone());
                        }
                    }
                }
                for tref in refs {
                    input_idents.insert(tref);
                }
            }
        }
        let output_idents: std::collections::BTreeSet<TypeRef> = match &sig.output {
            syn::ReturnType::Type(_, ty) => self.resolve_ty(ty).into_iter().collect(),
            syn::ReturnType::Default => std::collections::BTreeSet::new(),
        };
        self.sigs.push(SinkFnSig {
            module_path: self.current_module(),
            name,
            input_idents,
            output_idents,
            mut_outparam_idents,
            test_gated: fn_attrs_are_cfg_test(attrs),
            module_private,
        });
    }
}

/// Whether an attribute set carries a `#[cfg(test)]` /
/// `#[cfg(any(test, feature = "test-support"))]` gate (so the fn is
/// production-absent). Reuses the EXACT canonical recogniser the carrier-gate
/// guard uses.
fn fn_attrs_are_cfg_test(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|a| {
        a.path().is_ident("cfg")
            && a.parse_args::<proc_macro2::TokenStream>()
                .map(cfg_is_exactly_test_or_test_support)
                .unwrap_or(false)
    })
}

impl<'ast> syn::visit::Visit<'ast> for SinkFnCollector<'_> {
    fn visit_item_fn(&mut self, f: &'ast syn::ItemFn) {
        // Free fn: explicit item visibility governs reachability.
        let module_private = visibility_is_module_private(&f.vis);
        self.record(f.sig.ident.to_string(), &f.sig, &f.attrs, module_private);
        syn::visit::visit_item_fn(self, f);
    }

    fn visit_item_impl(&mut self, i: &'ast syn::ItemImpl) {
        // Track whether the enclosing impl is a trait impl: a trait-impl method
        // is reachable through the trait regardless of its (always-`Inherited`)
        // syntactic visibility, so it is NEVER module-private.
        let prev = self.in_trait_impl;
        self.in_trait_impl = i.trait_.is_some();
        syn::visit::visit_item_impl(self, i);
        self.in_trait_impl = prev;
    }

    fn visit_impl_item_fn(&mut self, f: &'ast syn::ImplItemFn) {
        // Inherent-impl method: its explicit visibility governs reachability. A
        // trait-impl method (no syntactic vis) rides the trait's visibility, so
        // it is never treated as module-private.
        let module_private = !self.in_trait_impl && visibility_is_module_private(&f.vis);
        self.record(f.sig.ident.to_string(), &f.sig, &f.attrs, module_private);
        syn::visit::visit_impl_item_fn(self, f);
    }

    fn visit_trait_item_fn(&mut self, f: &'ast syn::TraitItemFn) {
        // A trait METHOD declaration (default-bodied or required) is a surface
        // too — a `trait T { fn raw(&SurfaceMember) -> ExpandedField; }` form.
        // Its reachability rides the trait's visibility (a trait item carries no
        // visibility of its own), so it is NEVER module-private here.
        self.record(f.sig.ident.to_string(), &f.sig, &f.attrs, false);
        syn::visit::visit_trait_item_fn(self, f);
    }

    fn visit_item_mod(&mut self, m: &'ast syn::ItemMod) {
        if mod_is_cfg_test(&m.attrs) {
            return; // test submodule — production-unreachable
        }
        // GAP-7: an inline `mod ident {}` qualifies its fns as `<parent>::ident`,
        // so a `(file, fn)` allowlist entry does not match a same-named fn in a
        // different inline submodule (and the no-unsafe / classification scopes
        // are precise per inline mod).
        let child = format!("{}::{}", self.current_module(), m.ident);
        self.module_stack.push(child);
        syn::visit::visit_item_mod(self, m);
        self.module_stack.pop();
    }
}

/// Collect every production fn signature in the registered sink-scope files.
fn collect_sink_fn_sigs(name_to_ids: &NameDefIndex) -> Vec<SinkFnSig> {
    let resolve_index = name_index_with_seed_ids(name_to_ids);
    // The cross-file re-export index + the module-scoped intra-crate use-binding
    // PROOF index so a sink-fn param/return type written through a `pub` /
    // `pub(crate)` re-export path OR a private-`use` chain resolves to its real
    // def.
    let reexports = collect_reexport_index();
    let use_bindings = collect_use_binding_index();
    let mut out = Vec::new();
    for (rel, src) in production_src_files() {
        let module_path = module_path_for_rel(&rel);
        if !SINK_SCAN_PREFIXES
            .iter()
            .any(|p| module_path == *p || module_path.starts_with(&format!("{p}::")))
        {
            continue;
        }
        let file = match syn::parse_file(&src) {
            Ok(f) => f,
            Err(_) => continue,
        };
        let uses = collect_use_index(&file);
        let mut collector = SinkFnCollector {
            module_stack: vec![module_path.clone()],
            sigs: Vec::new(),
            in_trait_impl: false,
            uses,
            name_to_ids: &resolve_index,
            reexports: &reexports,
            use_bindings: &use_bindings,
        };
        syn::visit::Visit::visit_file(&mut collector, &file);
        out.extend(collector.sigs);
    }
    out.sort();
    out
}

/// THE policy half — parameterised over the collected fn sigs, the
/// TypeExpr-bearing OUTPUT closure, the forgeable-authority INPUT closure, and
/// the allowlist, so the self-test can drive it with synthetic records. A sig
/// FIRES iff:
///   (output channel reaches the TypeExpr-bearing set — return OR `&mut`
///    out-param) AND (some input ident is in the forgeable-authority set —
///    a forgeable raw-authority seed OR a WRAPPER type whose def transitively
///    reaches one) AND the `(module, fn)` pair is NOT on the sink-local
///    allowlist.
/// `#[cfg(test)]` sigs are dropped. A MODULE-PRIVATE fn (a bare `fn` /
/// `pub(self)` inherent method, never a trait-associated method) is dropped: it
/// is unreachable from a sibling module, so it cannot be a CROSS-sink boundary —
/// the forgeable-input → TypeExpr pairing stays confined to its own module.
fn cross_sink_raw_authority_violations(
    sigs: &[SinkFnSig],
    typeexpr_bearing: &std::collections::BTreeSet<TypeDefId>,
    forgeable_authority: &std::collections::BTreeSet<TypeDefId>,
) -> Vec<String> {
    let allow: std::collections::BTreeSet<(&str, &str)> =
        SINK_LOCAL_RAW_AUTHORITY_ALLOWLIST.iter().copied().collect();
    let chain_ids = sealed_construction_chain_ids();
    // A ref is "forgeable input" when it RESOLVES to a forgeable-authority id OR
    // a pre-admission construction-chain id (qualified identity).
    let is_forgeable_input = |tref: &TypeRef| -> bool {
        tref.resolved()
            .is_some_and(|id| forgeable_authority.contains(id) || chain_ids.contains(id))
    };
    let is_bearing = |tref: &TypeRef| -> bool {
        tref.resolved()
            .is_some_and(|id| typeexpr_bearing.contains(id))
    };
    let mut violations = Vec::new();
    for sig in sigs {
        if sig.test_gated {
            continue;
        }
        // Module-private fn: unreachable cross-sink (Rust visibility confines it
        // to its own module), so it is NOT a cross-sink boundary. The node-input
        // cores (`materialize_member_surface_node_core` /
        // `projected_expanded_shape_from_node_core`) are exactly this: private
        // cores the demand APIs resolve through internally.
        if sig.module_private {
            continue;
        }
        let output_bearing = sig.output_idents.iter().any(is_bearing);
        let mut_outparam_bearing = sig.mut_outparam_idents.iter().any(is_bearing);
        if !output_bearing && !mut_outparam_bearing {
            continue;
        }
        // INPUT side uses the SAME transitive field-closure discipline as the
        // output side: a param of a WRAPPER type whose def transitively reaches
        // a forgeable seed (`struct WrappedSurfaceMember { member: SurfaceMember }`)
        // is forgeable authority, not just a direct-seed param. ADDITIONALLY
        // (GAP-3 split): a PRE-admission construction-chain struct
        // (`SurfaceMemberCandidate` / `ResolvedMacroPayload` /
        // `ResolvedPayloadSurface`) taken DIRECTLY is forgeable — it bypassed the
        // `admit_published_member` policy gate, so it is NOT a policy-admitted
        // safe input. (The chain structs are excluded from the propagation
        // closure so they don't poison every wrapper, but a direct chain-struct
        // param still fires here.) The policy-admitted safe inputs
        // (`AdmittedPublishedMember` + the framework tokens) are NOT in the
        // closure and NOT chain structs, so a fn taking only those does not fire.
        let forgeable_input = sig.input_idents.iter().any(is_forgeable_input);
        if !forgeable_input {
            continue;
        }
        if allow.contains(&(sig.module_path.as_str(), sig.name.as_str())) {
            continue;
        }
        let channel = if output_bearing {
            "return type"
        } else {
            "`&mut` DTO out-param"
        };
        violations.push(format!(
            "`{}::{}` pairs a FORGEABLE raw-authority input (one of {:?}) with a `TypeExpr`-bearing \
             {channel} — a member/signature `TypeExpr` may cross the publication boundary ONLY from \
             an admitted publication token, never from a forgeable surface/member/node (directly OR \
             through a wrapper type that transitively reaches one). Route the input through the \
             sealed admitted-token chain (an `AdmittedPublishedMember` / `ResolvedPayloadSurface` / \
             `ResolvedVueSurface` token), or — if this is a genuine sink-local raiser/projector — \
             add `(\"{}\", \"{}\")` to SINK_LOCAL_RAW_AUTHORITY_ALLOWLIST.",
            sig.module_path,
            sig.name,
            sig.input_idents
                .iter()
                .filter(|tref| is_forgeable_input(tref))
                .map(|tref| tref.final_segment().to_string())
                .collect::<Vec<_>>(),
            sig.module_path,
            sig.name,
        ));
    }
    violations
}

/// The SEALED CONSTRUCTION-CHAIN structs (qualified by owning module): the
/// PRE-admission stages of the publication-authority chain
/// (`ResolvedMacroPayload` → `ResolvedPayloadSurface` → `SurfaceMemberCandidate`,
/// all minted BEFORE the `admit_published_member` policy gate at
/// `publication_authority.rs`). These are sealed (private fields + a private
/// `Seal`), so they are NOT pulled into the forgeable-authority CLOSURE via the
/// raw seed field they internally hold (a `member: SurfaceMember` /
/// `node: SemanticNodeId` field) — excluding them from the closure keeps a type
/// that merely WRAPS one of them from being marked forgeable through it. But
/// they are NOT policy-admitted: a sink fn taking a pre-admission chain struct
/// DIRECTLY and returning a `TypeExpr`-bearing value MUST fire (it bypassed the
/// admission gate). So the cross-sink violation check treats a chain-struct
/// PARAM as forgeable authority directly, while the closure does not propagate
/// through them.
///
/// Keyed `(owning_module_path, name)` so the bare-name collision
/// `ResolvedMacroPayload` (this sealed token in `publication_authority` vs the
/// unrelated DTO alias `ResolvedOutcome<Arc<MacroSurfaceDtos>>` in
/// `typeinfo::framework_surface::results`) is resolved by QUALIFIED identity:
/// only the `publication_authority` struct is a construction-chain stage; the
/// `results` alias is an OUTPUT DTO classified on its own merits (it is bearing).
const SEALED_CONSTRUCTION_CHAIN_STRUCTS: &[(&str, &str)] = &[
    (
        "crate::meta_resolve::projectors::publication_authority",
        "ResolvedMacroPayload",
    ),
    (
        "crate::meta_resolve::projectors::publication_authority",
        "ResolvedPayloadSurface",
    ),
    (
        "crate::meta_resolve::projectors::publication_authority",
        "SurfaceMemberCandidate",
    ),
];

/// The POLICY-ADMITTED safe publication inputs (qualified by owning module): the
/// SOLE set a sink fn may take as a "safe" forgeable-adjacent input WITHOUT
/// firing — the policy-admitted publication token `AdmittedPublishedMember`
/// (minted only by `admit_published_member` AFTER the policy gate) plus the two
/// per-framework sealed resolved-surface tokens `ResolvedVueSurface` /
/// `SvelteResolvedSurface` (minted only inside their `vue_exec` / `svelte_exec`
/// owner). A sink fn whose only forgeable-adjacent input is one of these is
/// routing through the admitted-token chain and does not fire; ANY OTHER
/// forgeable input (a raw seed, a wrapper of one, OR a pre-admission chain
/// struct) does fire.
const POLICY_ADMITTED_SAFE_INPUTS: &[(&str, &str)] = &[
    (
        "crate::meta_resolve::projectors::publication_authority",
        "AdmittedPublishedMember",
    ),
    (
        "crate::typeinfo::framework_surface::vue_exec",
        "ResolvedVueSurface",
    ),
    (
        "crate::typeinfo::framework_surface::svelte_exec",
        "SvelteResolvedSurface",
    ),
];

/// The construction-chain structs as `TypeDefId`s (the closure exclusion + the
/// cross-sink direct-forgeable-input check, which now key on QUALIFIED identity).
/// A chain id is treated as forgeable when its `TypeDefId` is taken DIRECTLY as a
/// sink param (pre-admission), but is excluded from the closure's transitive
/// propagation (it does not poison every type that wraps one). The qualified
/// `(module, name)` distinguishes the sealed `publication_authority`
/// `ResolvedMacroPayload` token from the unrelated bearing `results` DTO alias.
fn sealed_construction_chain_ids() -> std::collections::BTreeSet<TypeDefId> {
    SEALED_CONSTRUCTION_CHAIN_STRUCTS
        .iter()
        .map(|(m, n)| TypeDefId::new(*m, *n))
        .collect()
}

/// The policy-admitted safe inputs as `TypeDefId`s (the closure exclusion + the
/// "safe sink-fn input" set), keyed by qualified identity.
fn policy_admitted_safe_input_ids() -> std::collections::BTreeSet<TypeDefId> {
    POLICY_ADMITTED_SAFE_INPUTS
        .iter()
        .map(|(m, n)| TypeDefId::new(*m, *n))
        .collect()
}

/// FAIL-CLOSED ANTI-VACUITY check: every sanctioned safe-input + construction-
/// chain `(module, name)` MUST match a collected `TypeDefId` EXACTLY (the token
/// must be defined in its named module). A token that is missing, or defined
/// ONLY in a DIFFERENT module (renamed / moved), FAILS — a stale exemption never
/// silently un-exempts.
///
/// Under MODULE-QUALIFIED identity there is no longer a "collision accepted
/// because the other def is bearing" carve-out: a bare name shared by two
/// modules is simply two DISTINCT `TypeDefId`s, each classified on its own merits
/// (the sealed `publication_authority` `ResolvedMacroPayload` token vs the
/// bearing `results` DTO alias resolve to distinct ids; an ambiguous UNQUALIFIED
/// reference to such a name is caught fail-closed at the boundary, not blessed
/// here). So this check is purely the anti-vacuity rail on the qualified set.
fn qualified_safe_input_collision_violations(name_to_ids: &NameDefIndex) -> Vec<String> {
    let mut out = Vec::new();
    let sanctioned: Vec<(&str, &str)> = SEALED_CONSTRUCTION_CHAIN_STRUCTS
        .iter()
        .chain(POLICY_ADMITTED_SAFE_INPUTS.iter())
        .copied()
        .collect();
    let empty = std::collections::BTreeSet::new();
    for (module, name) in &sanctioned {
        let ids = name_to_ids.get(*name).unwrap_or(&empty);
        if ids.is_empty() {
            out.push(format!(
                "qualified safe-input `{module}::{name}` has NO definition in the read type-def \
                 roots — the token was renamed / moved without updating the qualified safe-input \
                 set (anti-vacuity)"
            ));
            continue;
        }
        let want = TypeDefId::new(*module, *name);
        if !ids.contains(&want) {
            let observed: Vec<&str> = ids.iter().map(|id| id.module.as_str()).collect();
            out.push(format!(
                "qualified safe-input `{module}::{name}` is NOT defined in its sanctioned module \
                 (observed in {observed:?}) — the token moved; update the qualified set to its real \
                 module"
            ));
        }
    }
    out.sort();
    out.dedup();
    out
}

/// The raw graph-HANDLE input seed `TypeDefId` — the ONE seed that does NOT
/// propagate the wrapper field-closure. A `SemanticNodeId` is the UNIVERSAL graph
/// handle embedded (through caches / maps / the `TypeExpr` payload) in nearly
/// every infrastructure type, so propagating the wrapper closure through it would
/// swallow the whole type universe. A bare `SemanticNodeId` param still fires
/// DIRECTLY (it is an [`INPUT_AUTHORITY_SEEDS`] seed); a sink-internal node core
/// taking it is closed by module-privacy + the demand-API seam.
fn non_propagating_input_seed_id() -> TypeDefId {
    TypeDefId::new("crate::semantic_query", "SemanticNodeId")
}

/// Compute the set of all type names that are FORGEABLE raw-authority by
/// STRUCTURAL FIELD-CLOSURE, analogous to [`typeexpr_bearing_closure`]: seed
/// with [`INPUT_AUTHORITY_SEEDS`], then iterate to a fixed point — a NON-bearing
/// type name is added when any PROPAGATING forgeable type is among its outgoing
/// refs (a field / variant / alias RHS reaching the set). So a wrapper
/// `struct WrappedSurfaceMember { member: SurfaceMember }` lands in the set
/// WITHOUT a spelled-name entry, so the INPUT side classifies a wrapper of a
/// forgeable seed with the same field-closure discipline as the OUTPUT side
/// (a direct-param-ident-only input check would miss the wrapper).
///
/// Two soundness fences keep this from over-approximating to the whole type
/// universe (every type transitively reaches the ubiquitous `SemanticNodeId` /
/// `TypeExpr`):
///   1. A TypeExpr-BEARING type is NEVER added and NEVER traversed — it is an
///      OUTPUT payload (classified by [`typeexpr_bearing_closure`]), not a
///      forgeable raw-surface INPUT wrapper. (`SurfaceView` is a bearing seed:
///      it stays a DIRECT seed — a `&SurfaceView` param fires — but does not
///      propagate wrapper-hood into its containers.)
///   2. The universal graph handle [`NON_PROPAGATING_INPUT_SEED`]
///      (`SemanticNodeId`) does NOT propagate: a type is a forgeable wrapper
///      because it carries a raw SURFACE / MEMBER subject (`SurfaceMember`,
///      `TypeInfoSurfaceMember`, …), not because it holds a node ordinal deep in
///      a cache. The bare handle still fires as a DIRECT seed.
///
/// The sealed-construction-chain structs AND the policy-admitted safe inputs are
/// the SANCTIONED sealed carriers — each deliberately carries a forgeable seed
/// inside (an `AdmittedPublishedMember` holds a `SurfaceMember` /
/// `ProjectionCursor`; a `SurfaceMemberCandidate` holds a `SurfaceMember`), but a
/// sealed carrier is NOT a forgeable raw-surface WRAPPER for the purposes of
/// transitive propagation. They are EXCLUDED from the closure (a type that merely
/// holds one is not marked forgeable through it). NOTE the split: the
/// construction-chain structs are excluded from the CLOSURE here, but a sink fn
/// taking one DIRECTLY still fires (the cross-sink check treats a pre-admission
/// chain-struct param as forgeable authority) — only [`POLICY_ADMITTED_SAFE_INPUTS`]
/// is a genuinely-safe sink-fn input.
///
/// GAP-2 — the dual-bearing defense is "DIRECT carve-out + TRANSITIVE tripwire",
/// NOT a transitive fence here: a TypeExpr-bearing type is excluded from the
/// forgeable INPUT set UNLESS its OWN def DIRECTLY co-holds a resolution-authority
/// propagating seed field. That DIRECT carve-out ([`direct_resolution_authority_holders`])
/// keeps a DUAL type — one that bears `TypeExpr` AND directly holds a propagating
/// seed (`struct Dual { m: SurfaceMember, t: TypeExpr }`, or a resolution-authority
/// wrapper `W { v: SurfaceView, … }` reaching `TypeExpr` through a nested
/// sub-struct) — forgeable: forging it yields authority regardless of it also
/// being an output payload. The carve-out stays DIRECT deliberately (a transitive
/// reach over-fires ~20 false positives, because the output seed `TypeExpr` and an
/// input seed `IndexSignature` share structure). SOUNDNESS for the deeper
/// (nested-seed) case is the SEPARATE TRANSITIVE TRIPWIRE
/// [`dual_bearing_violations`] (its seed side is the full [`raw_authority_reach_closure`]):
/// it asserts the real-tree premise that no production type co-holds a propagating
/// seed AND a DIRECT `TypeExpr` field, so any hit is investigated — never silently
/// dropped. The fence here is therefore DIRECT; the transitive guarantee rides the
/// tripwire.
fn forgeable_authority_closure(
    defs: &BTreeMap<TypeDefId, TypeDefRefs>,
    typeexpr_bearing: &std::collections::BTreeSet<TypeDefId>,
) -> std::collections::BTreeSet<TypeDefId> {
    // GAP-2: the DUAL-BEARING carve-out set — types whose OWN def DIRECTLY holds a
    // RESOLUTION-AUTHORITY propagating seed FIELD (a raw surface/member subject
    // that itself NEVER bears `TypeExpr` — `SurfaceMember` / `SurfaceView` /
    // `TypeInfoSurface*` / `VueMacroSurface` / `ProjectionCursor`). A bearing type
    // in this set is a DUAL-BEARING wrapper (it both bears `TypeExpr` AND directly
    // carries resolution authority) that must STAY forgeable — forging it yields
    // authority even though it is also a bearing output. The seed side is DIRECT
    // (a `Dual { m: SurfaceMember, t: TypeExpr }`, or a resolution-authority
    // wrapper `W { v: SurfaceView, … }`), NOT a transitive reach: an infra/cache
    // type that only buries an RA-seed deep behind a sealed key (`ShapeCacheKey`)
    // or the whole `VerterHost` does NOT directly co-hold one and is correctly
    // skipped by the bearing fence. CRITICAL: a propagating seed that is ITSELF
    // `TypeExpr`-bearing (`IndexSignature` directly holds two `TypeExpr` fields —
    // it is PART of `TypeExpr`'s own structure) is NOT a resolution-authority
    // subject, so a type holding it (`ObjectMember`, `TypeExpr`) is NOT a
    // dual-bearing carve-out.
    let bearing_exempt = direct_resolution_authority_holders(defs, typeexpr_bearing);
    forgeable_closure_inner(defs, typeexpr_bearing, &bearing_exempt)
}

/// The set of `TypeDefId`s whose OWN def DIRECTLY references (as an immediate
/// field ref, possibly inside `Vec`/`Option`/`Arc`/… — `type_segment_refs`
/// already separated those) a RESOLUTION-AUTHORITY propagating seed: a propagating
/// seed (excludes the `SemanticNodeId` handle) that is NOT itself `TypeExpr`-bearing.
/// This is the GAP-2 dual-bearing carve-out: a bearing type that DIRECTLY
/// co-holds such a seed must stay forgeable. The seed side is DIRECT (a genuine
/// forgeable wrapper co-locates the raw subject), so an infra type that only
/// transitively buries an RA-seed is not swept in.
fn direct_resolution_authority_holders(
    defs: &BTreeMap<TypeDefId, TypeDefRefs>,
    typeexpr_bearing: &std::collections::BTreeSet<TypeDefId>,
) -> std::collections::BTreeSet<TypeDefId> {
    let node_handle = non_propagating_input_seed_id();
    let ra_seeds: std::collections::BTreeSet<TypeDefId> = input_authority_seed_ids()
        .into_iter()
        .filter(|id| *id != node_handle)
        .filter(|id| !typeexpr_bearing.contains(id))
        .collect();
    defs.iter()
        .filter(|(_, def)| {
            def.refs
                .iter()
                .any(|r| r.resolved().is_some_and(|id| ra_seeds.contains(id)))
        })
        .map(|(id, _)| id.clone())
        .collect()
}

/// The set of `TypeDefId`s that TRANSITIVELY reach a RESOLUTION-AUTHORITY
/// propagating seed (a propagating seed — excludes the `SemanticNodeId` handle —
/// that is NOT itself `TypeExpr`-bearing), following resolved field/variant refs
/// through sub-wrappers. Unlike [`direct_resolution_authority_holders`] (a single
/// DIRECT-field hop, the carve-out that must stay direct to avoid the 20-FP
/// over-fire), this is a full transitive reach used ONLY by the dual-bearing
/// TRIPWIRE seed side: the tripwire needs SOUNDNESS, not FP-freedom (it asserts a
/// real-tree premise; any hit is investigated, never silently exempted), so a
/// transitive reach is the correct shape there. NO bearing fence is applied
/// (this is a pure reachability over the field graph, mirroring the forgeable
/// reach without the skip-bearing fence).
fn raw_authority_reach_closure(
    defs: &BTreeMap<TypeDefId, TypeDefRefs>,
    typeexpr_bearing: &std::collections::BTreeSet<TypeDefId>,
) -> std::collections::BTreeSet<TypeDefId> {
    let node_handle = non_propagating_input_seed_id();
    let ra_seeds: std::collections::BTreeSet<TypeDefId> = input_authority_seed_ids()
        .into_iter()
        .filter(|id| *id != node_handle)
        .filter(|id| !typeexpr_bearing.contains(id))
        .collect();
    // Seed the reach set with the RA-seeds themselves, then iterate: a def
    // reaches the set when any RESOLVED ref is already in it.
    let mut reach: std::collections::BTreeSet<TypeDefId> = ra_seeds;
    loop {
        let mut added = false;
        for (id, def) in defs {
            if reach.contains(id) {
                continue;
            }
            if def
                .refs
                .iter()
                .any(|r| r.resolved().is_some_and(|t| reach.contains(t)))
            {
                reach.insert(id.clone());
                added = true;
            }
        }
        if !added {
            break;
        }
    }
    reach
}

/// The shared forgeable-authority fixpoint. A type is added when it carries a
/// PROPAGATING forgeable subject (fence-2: not merely a node ordinal) OR is a
/// structurally-equivalent newtype/alias of a raw-node singleton (FIX-8a). A
/// TypeExpr-bearing type is SKIPPED (the bearing fence) UNLESS it is in
/// `bearing_exempt` (the GAP-2 dual-bearing carve-out — a type DIRECTLY holding a
/// resolution-authority seed). The sealed carriers (policy-admitted +
/// construction-chain) are excluded throughout.
fn forgeable_closure_inner(
    defs: &BTreeMap<TypeDefId, TypeDefRefs>,
    typeexpr_bearing: &std::collections::BTreeSet<TypeDefId>,
    bearing_exempt: &std::collections::BTreeSet<TypeDefId>,
) -> std::collections::BTreeSet<TypeDefId> {
    let closure_excluded: std::collections::BTreeSet<TypeDefId> = policy_admitted_safe_input_ids()
        .into_iter()
        .chain(sealed_construction_chain_ids())
        .collect();
    let node_handle = non_propagating_input_seed_id();
    // The direct seeds are always forgeable (even a bearing seed like
    // `SurfaceView` — a direct `&SurfaceView` param fires).
    let mut forgeable: std::collections::BTreeSet<TypeDefId> = input_authority_seed_ids();
    // The PROPAGATING subset: the seeds that turn a container into a wrapper.
    // Excludes the universal handle so the closure does not swallow every
    // node-bearing infrastructure type.
    let mut propagating: std::collections::BTreeSet<TypeDefId> = input_authority_seed_ids()
        .into_iter()
        .filter(|id| *id != node_handle)
        .collect();
    // FIX-8a: the SINGLETON-EQUIVALENT set — `TypeDefId`s that, when a def IS
    // EXACTLY one of them (a single-field newtype / a transparent alias, a THIN
    // RENAME of the raw subject), make the renaming type forgeable authority too.
    // Seeded with the universal graph handle `SemanticNodeId` id (which does NOT
    // propagate through MULTI-field containers — the 28-FP fix — but a newtype /
    // alias that IS structurally just it is itself a raw-node carrier), and grown
    // with each such newtype found so a newtype-of-a-newtype also matches.
    let mut node_singleton: std::collections::BTreeSet<TypeDefId> =
        [node_handle.clone()].into_iter().collect();
    loop {
        let mut added = false;
        for (id, def) in defs {
            if forgeable.contains(id) {
                continue;
            }
            // Soundness fence 1 (GAP-2 — TRANSITIVE): a TypeExpr-bearing type is
            // skipped ONLY if it does NOT transitively reach a propagating
            // forgeable seed. A dual-bearing wrapper (in `bearing_exempt`) is
            // NOT skipped — forging it yields authority even though it is also a
            // bearing output. (`bearing_exempt` empty + `typeexpr_bearing` empty
            // is the reach-closure pass, which never skips for bearing.)
            if typeexpr_bearing.contains(id) && !bearing_exempt.contains(id) {
                continue;
            }
            // The sealed carriers (policy-admitted safe inputs + pre-admission
            // construction-chain structs) are never pulled into the closure via
            // a field even though they hold a forgeable seed (the split: a
            // chain-struct still fires when taken DIRECTLY as a sink param, via
            // the cross-sink check — but it does not poison every wrapper here).
            if closure_excluded.contains(id) {
                continue;
            }
            // A type is a forgeable wrapper iff a RESOLVED ref carries a
            // PROPAGATING forgeable subject (a raw surface / member), not merely a
            // node ordinal buried in a cache (soundness fence 2) — OR it is a
            // structurally-equivalent newtype / alias of a raw-node singleton
            // (FIX-8a): its refs, after removing pure container / smart-pointer
            // shells, are EXACTLY one singleton-equivalent id.
            let carries_propagating = def
                .refs
                .iter()
                .any(|r| r.resolved().is_some_and(|t| propagating.contains(t)));
            let is_node_newtype = def_is_singleton_rename_of(def, &node_singleton);
            if carries_propagating || is_node_newtype {
                forgeable.insert(id.clone());
                // A non-bearing wrapper also propagates (a wrapper of a wrapper
                // is still a forgeable wrapper). A dual-bearing wrapper kept by
                // the GAP-2 carve-out propagates too — it genuinely carries the
                // forgeable seed.
                propagating.insert(id.clone());
                // A newtype/alias structurally equal to the raw handle is itself
                // a raw-node carrier — a FURTHER newtype/alias of IT is too, so
                // grow the singleton-equivalent set (the recursive case). Only
                // grow it for the singleton-rename path: a multi-field wrapper
                // that merely CONTAINS a forgeable seed is a propagating wrapper
                // but not a thin node rename, so it must NOT seed the singleton
                // set (else a wrapper-of-wrapper would relax the 28-FP fence).
                if is_node_newtype {
                    node_singleton.insert(id.clone());
                }
                added = true;
            }
        }
        if !added {
            break;
        }
    }
    // Defensively remove any sealed carrier a seed collision pulled in — a
    // policy-admitted safe input is NEVER forgeable authority (it is the allowed
    // admitted input), and a construction-chain struct fires via the cross-sink
    // DIRECT check, not via closure membership (so it must not be in `forgeable`,
    // which would make every wrapper of it propagate too).
    for id in &closure_excluded {
        forgeable.remove(id);
    }
    forgeable
}

/// Pure container / smart-pointer / transparent-wrapper idents stripped before
/// the FIX-8a singleton-rename test. A `struct W(Arc<SemanticNodeId>)` /
/// `type W = Box<SemanticNodeId>` is still a thin transparent rename of the raw
/// handle (the wrapper carries no OTHER subject), so the structural-equivalence
/// test removes these and checks the remainder. PRIMITIVES are NOT stripped: a
/// `struct Cache { node: SemanticNodeId, gen: u64 }` carries a node AMONG OTHER
/// (primitive) fields — it is genuinely multi-field, so its remainder
/// `{SemanticNodeId, u64}` is NOT a singleton and it stays non-propagating
/// (preserving the 28-false-positive fix).
const TRANSPARENT_WRAPPER_IDENTS: &[&str] = &[
    "Vec", "Option", "Box", "Arc", "Rc", "Cow", "RefCell", "Cell", "Mutex", "RwLock",
];

/// FIX-8a: is `def` a SINGLE-subject NEWTYPE / transparent ALIAS whose only
/// non-container type ref is exactly one name in `singletons`? (A
/// `struct W(SemanticNodeId)`, `type W = SemanticNodeId`, or
/// `struct W(Arc<SemanticNodeId>)` — a thin rename of the raw subject.) A
/// multi-field type holding the subject among OTHER fields (incl. primitive
/// fields) has a remainder larger than the singleton and returns `false`.
fn def_is_singleton_rename_of(
    def: &TypeDefRefs,
    singletons: &std::collections::BTreeSet<TypeDefId>,
) -> bool {
    let container: std::collections::BTreeSet<&str> =
        TRANSPARENT_WRAPPER_IDENTS.iter().copied().collect();
    // The non-container refs (the actual subject(s) the type renames). Container
    // shells are stripped by their FINAL segment (std `Vec`/`Arc`/… resolve to
    // `Unresolved`, so they are recognized by name).
    let subjects: Vec<&TypeRef> = def
        .refs
        .iter()
        .filter(|r| !container.contains(r.final_segment()))
        .collect();
    // EXACTLY one subject, and it RESOLVES to a singleton-equivalent raw-node id.
    // A type with zero subjects (an empty / unit-ish def) or two+ subjects, or a
    // subject that does not resolve to a singleton id, is NOT a thin rename.
    subjects.len() == 1
        && subjects
            .iter()
            .all(|r| r.resolved().is_some_and(|id| singletons.contains(id)))
}

/// FIX-8b: every DUAL-BEARING production type — one whose OWN def, in the SAME
/// struct / enum, co-holds BOTH a PROPAGATING forgeable input-authority seed
/// field (a `SurfaceMember` / `SurfaceView` / `VueMacroSurface` / `IndexSignature`
/// / … field, possibly inside a `Vec`/`Option`/`Arc`/… wrapper) AND a DIRECT
/// `TypeExpr` field.
///
/// This is the EXACT invariant that keeps the
/// [`forgeable_authority_closure`] fence-1 (skip-bearing-output-as-input) sound:
/// the genuinely-forgeable SURFACE/MEMBER seeds all carry their member VALUES as
/// `SemanticNodeId` (reference-style — `SurfaceMember.value: SemanticNodeId`,
/// `TypeInfoSurfaceMember.value: SemanticNodeId`, `IndexSignature.{key,value}:
/// SemanticNodeId`), NEVER as a co-located `TypeExpr`. The seed → `TypeExpr`
/// materialization is always input → output THROUGH a fn, never a struct holding
/// both. If a future DTO refactor ever FOLDS a seed-bearing wrapper that also
/// carries a raised `TypeExpr` into ONE struct (`struct Dual { m: SurfaceMember,
/// t: TypeExpr }`), fence-1 would silently drop it from the forgeable INPUT set
/// (it is now also a bearing OUTPUT), so a `fn(&Dual) -> X` would pass while
/// `Dual` carries forgeable authority — this check surfaces that LOUDLY.
///
/// The check keys on the type's OWN DIRECT `TypeExpr` field (a literal
/// `TypeExpr` among its refs) for the OUTPUT side, and on a TRANSITIVE
/// raw-authority reach for the SEED side (GAP-2): a type whose OWN def directly
/// holds a `TypeExpr` field AND TRANSITIVELY reaches a resolution-authority seed
/// (DIRECTLY, or through a sub-wrapper — `struct Raw { member: SurfaceMember }`
/// nested inside `struct NestedDual { raw: Raw, t: TypeExpr }`) FIRES. The seed
/// side is TRANSITIVE because the tripwire only needs SOUNDNESS, not FP-freedom
/// (it asserts a real-tree premise — RA-seeds carry their member values as
/// `SemanticNodeId`, never a co-located `TypeExpr` — so any hit is investigated,
/// never silently exempted); the 20-FP problem that forces the
/// [`direct_resolution_authority_holders`] carve-out to stay DIRECT does not
/// apply here. A type whose `TypeExpr` reach is itself the transitive part but
/// has NO direct `TypeExpr` field (the `SemanticNodeData` / `ObjectMember` shape,
/// where the seed member value is a `SemanticNodeId`) does NOT fire — the DIRECT
/// `TypeExpr` field is required. Excludes the admitted tokens (the sanctioned
/// carrier) and the direct [`INPUT_AUTHORITY_SEEDS`] themselves.
fn dual_bearing_violations(
    defs: &BTreeMap<TypeDefId, TypeDefRefs>,
    typeexpr_bearing: &std::collections::BTreeSet<TypeDefId>,
) -> Vec<String> {
    let seeds = input_authority_seed_ids();
    // The sanctioned-carrier exemption is keyed by QUALIFIED `(module, name)`
    // identity — the policy-admitted safe inputs ∪ the pre-admission
    // construction-chain structs — NOT a bare name. A wrong-module same-name type
    // (a forged `crate::evil::AdmittedPublishedMember`) is therefore NOT exempted;
    // only the real sanctioned token at its sanctioned module is. This mirrors the
    // already-qualified `seeds.contains(id)` check below.
    let sanctioned_carriers: std::collections::BTreeSet<TypeDefId> =
        policy_admitted_safe_input_ids()
            .into_iter()
            .chain(sealed_construction_chain_ids())
            .collect();
    let type_expr_id = TypeDefId::new("verter_type_expr", "TypeExpr");
    // GAP-2: the SEED side is a TRANSITIVE raw-authority reach (a type reaching an
    // RA-seed through a sub-wrapper still co-holds forgeable authority).
    let reach = raw_authority_reach_closure(defs, typeexpr_bearing);
    let mut out = Vec::new();
    for (id, def) in defs {
        // SEED side (TRANSITIVE): the type transitively reaches a resolution-
        // authority seed (directly or through a sub-wrapper).
        let reaches_seed = reach.contains(id);
        // DIRECT `TypeExpr` field, co-located in the SAME def. This is the
        // audited invariant: a seed-reaching type whose own def ALSO names
        // `TypeExpr` directly. The seeds carry their members as `SemanticNodeId`,
        // so today no type co-holds both.
        let has_direct_type_expr_field =
            def.refs.iter().any(|r| r.resolved() == Some(&type_expr_id));
        if !reaches_seed || !has_direct_type_expr_field {
            continue;
        }
        if sanctioned_carriers.contains(id) {
            continue; // sanctioned carrier (QUALIFIED identity — wrong-module same-name fires)
        }
        if seeds.contains(id) {
            continue; // a direct seed, not a surprise dual-bearing wrapper
        }
        let name = &id.name;
        out.push(format!(
            "`{name}` co-holds (transitively) a resolution-authority seed AND a DIRECT `TypeExpr` \
             field (a DUAL-BEARING type) — this makes the forgeable-authority closure's \
             skip-bearing-output-as-input fence UNSOUND: `{name}` would be silently dropped from \
             the forgeable INPUT set because it is also a bearing OUTPUT, so a \
             `fn(&{name}) -> <bearing>` would pass while carrying forgeable authority. Either route \
             the carrier through an admitted token, or split the seed and the `TypeExpr` field \
             into separate types"
        ));
    }
    out.sort();
    out
}

/// Known std / core / common-crate container + smart-pointer idents that appear
/// in sink-fn return types and are NEVER a TypeExpr-bearing DTO themselves (the
/// field-closure correctly classifies the WRAPPED type via `type_idents`
/// flattening — these are the wrapper shells). A `Vec<TypeExpr>` return already
/// lists `TypeExpr` among its idents, so the wrapper ident itself need not be in
/// `defs`.
const KNOWN_CONTAINER_OUTPUT_IDENTS: &[&str] = &[
    "Vec",
    "Option",
    "Box",
    "Arc",
    "Rc",
    "Result",
    "String",
    "HashMap",
    "HashSet",
    "BTreeMap",
    "BTreeSet",
    "FxHashMap",
    "FxHashSet",
    "Cow",
    "Self",
];

/// Known NON-DTO external types that appear in sink-fn return types, are defined
/// OUTSIDE the read file set, and are NOT TypeExpr-bearing. Recorded here
/// deliberately (the fail-closed completeness check below would otherwise flag
/// them as "unclassifiable"): each is a non-bearing handle / span / id / scope /
/// enum the closure never needs to follow. A NEW PascalCase output type NOT in
/// `defs` and NOT here FAILS the completeness check — so a TypeExpr-bearing
/// wrapper DTO whose def home is unread surfaces loudly rather than silently
/// passing as a non-bearing leaf.
/// Known NON-DTO output idents, QUALIFIER-AWARE (F2): each entry pairs a name
/// with its non-field-bearing [`NonAuthorityCategory`] (carrying the approved
/// qualified homes). An `Unresolved` output ref is exempted ONLY when its written
/// PATH agrees with the category ([`non_authority_category_exempts`]) — NEVER by
/// bare final segment, so a forged `evil::Span` output fires fail-closed and a
/// same-name COLLECTED def is never blessed.
const KNOWN_NON_DTO_OUTPUT_IDENTS: &[(&str, NonAuthorityCategory)] = &[
    // Container / smart-pointer / assoc-type idents not covered by
    // KNOWN_CONTAINER_OUTPUT_IDENTS (DashMap / RwLock from concurrent caches;
    // `Key` / `Value` assoc-type names; `Discriminant` / `Any` from std). A bare
    // one-segment form is benign; a qualified form is approved under the listed
    // crate/std homes.
    (
        "DashMap",
        NonAuthorityCategory::GenericOrAssocOrStd(&["dashmap"]),
    ),
    (
        "RwLock",
        NonAuthorityCategory::GenericOrAssocOrStd(&["parking_lot", "std::sync", "tokio::sync"]),
    ),
    ("Key", NonAuthorityCategory::GenericOrAssocOrStd(&["Self"])),
    (
        "Value",
        NonAuthorityCategory::GenericOrAssocOrStd(&["Self"]),
    ),
    (
        "Discriminant",
        NonAuthorityCategory::GenericOrAssocOrStd(&["std::mem", "core::mem", "Self"]),
    ),
    (
        "Any",
        NonAuthorityCategory::GenericOrAssocOrStd(&["std::any", "core::any"]),
    ),
    // Non-bearing diagnostic / span / id / context / scope types defined
    // outside the read seed homes. VERIFIED non-bearing (none carries a
    // `TypeExpr` field): `MacroExpansionDiagnostics` carries macro_kind /
    // macro_index / Vec<ExpansionDiagnostic> / exactness / execution_status;
    // `Span` is a byte range; `ResolverContext` is a trait; `PreparedTypeDecl`
    // IS bearing and is read via the widened EXTERNAL roots (NOT here).
    (
        "MacroExpansionDiagnostics",
        NonAuthorityCategory::ExternalNonAuthority(&["verter_semantic::analysis::component_meta"]),
    ),
    (
        "Span",
        NonAuthorityCategory::ExternalNonAuthority(&["verter_span"]),
    ),
    (
        "ResolverContext",
        NonAuthorityCategory::SealedTraitBound(&["crate::resolver_core"]),
    ),
    (
        "DeclarationId",
        NonAuthorityCategory::ExternalNonAuthority(&["verter_semantic::analysis::type_eval"]),
    ),
    // Framework-surface WIRE response / error / audit-envelope types — proto /
    // audit DTOs carrying GRAPH nodes (a shallow projection), NOT the Rust
    // `verter_type_expr::TypeExpr` payload, so non-bearing for this closure. The
    // approved home is the CRATE the sink imports the type FROM: the proto wire
    // types live in `verter_protocol::typeinfo::graph`, the audit envelope in
    // `verter_audit`. A `use … as Alias` import resolves the sink-fn return ident
    // to its imported path, so the exemption matches the import qualifier against
    // the real external home (none of these has a read-root collision).
    (
        "TypeInfoGraphResponse",
        NonAuthorityCategory::ExternalNonAuthority(&["verter_protocol::typeinfo::graph"]),
    ),
    (
        "TypeInfoRequestError",
        NonAuthorityCategory::ExternalNonAuthority(&["verter_protocol::typeinfo::graph"]),
    ),
    (
        "AuditedResult",
        NonAuthorityCategory::ExternalNonAuthority(&["verter_audit"]),
    ),
];

/// The non-field-bearing CATEGORY of a known non-DTO output NAME, if any.
fn non_dto_output_category(name: &str) -> Option<NonAuthorityCategory> {
    KNOWN_NON_DTO_OUTPUT_IDENTS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, c)| *c)
}

/// FAIL-CLOSED completeness: collect every OUTPUT / `&mut`-out-param PascalCase
/// ident a (non-test, non-module-private) sink fn returns that is NOT
/// classifiable — neither RESOLVED to a concrete `TypeDefId` (the field-closure
/// could decide its bearing-ness) nor a known container
/// ([`KNOWN_CONTAINER_OUTPUT_IDENTS`]) nor a known non-DTO external
/// ([`KNOWN_NON_DTO_OUTPUT_IDENTS`], QUALIFIER-AWARE). An `Unresolved` PascalCase
/// ref that is none of those FAILS fail-closed — a bearing-wrapper DTO whose def
/// home is unread (or a forged-qualifier bare name) surfaces loudly. A
/// single-uppercase generic param (`T`, `K`) and a lowercase primitive (`u32`,
/// `bool`) are never flagged.
fn unclassifiable_output_idents(
    sigs: &[SinkFnSig],
    name_to_ids: &NameDefIndex,
) -> Vec<(String, String)> {
    let container: std::collections::BTreeSet<&str> =
        KNOWN_CONTAINER_OUTPUT_IDENTS.iter().copied().collect();
    let mut out = Vec::new();
    for sig in sigs {
        if sig.test_gated || sig.module_private {
            continue;
        }
        for tref in sig
            .output_idents
            .iter()
            .chain(sig.mut_outparam_idents.iter())
        {
            // A RESOLVED ref is classifiable — the closure could decide it.
            if tref.resolved().is_some() {
                continue;
            }
            let id = tref.final_segment();
            // Only PascalCase idents (a leading uppercase + at least one more
            // char) can be a DTO; a primitive (`u32`, `str`) is lowercase, a
            // generic param (`T`) is a single char.
            let mut chars = id.chars();
            let Some(first) = chars.next() else {
                continue;
            };
            if !first.is_ascii_uppercase() || id.len() < 2 {
                continue;
            }
            if container.contains(id) {
                continue;
            }
            if let Some(category) = non_dto_output_category(id) {
                if non_authority_category_exempts(tref, category, name_to_ids) {
                    continue;
                }
            }
            out.push((format!("{}::{}", sig.module_path, sig.name), id.to_string()));
        }
    }
    out.sort();
    out.dedup();
    out
}

/// Known NON-AUTHORITY input types (GAP-6): PascalCase param types that appear on
/// a TypeExpr-bearing sink boundary, are defined OUTSIDE the read type-def roots,
/// and are NOT forgeable raw authority — a host handle, a context/ctx, a
/// snapshot, a config, a scope, an env, a diagnostic, an id, a span, an enum
/// flag. Recorded here deliberately (the input fail-closed check below would
/// otherwise flag them as "unclassifiable"): each is a non-authority external the
/// forgeable closure never needs to follow, justified inline. A NEW PascalCase
/// input type on a bearing boundary that is NOT in `defs`, NOT a known container,
/// NOT a safe token / chain struct, and NOT here FAILS — so a forgeable wrapper
/// whose def home is unread surfaces loudly rather than passing as a benign
/// external.
/// A non-field-bearing CATEGORY for a [`KnownNonAuthorityInput::Category`] entry
/// — a name that is benign as a sink input NOT because a `(module, name)` def
/// proves it (its def home is not a read root, or it has no struct def at all),
/// but because of WHAT it is: a sealed trait bound, a generic / associated-type
/// name, or a non-collected external value type.
///
/// Each variant carries the APPROVED qualified module-path prefixes its name may
/// be written under. A CATEGORY exempts an UNRESOLVED ref ONLY when the ref's
/// PATH agrees with the category (the [`non_authority_category_exempts`] rule),
/// NEVER by bare final segment — so a forged `evil::Span` does NOT match the
/// benign `Span` category, and a same-name COLLECTED def is never blessed by a
/// bare-name exemption (the real def classifies it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NonAuthorityCategory {
    /// A TRAIT bound surfacing as a param ident (`&impl T` / `&dyn T`) — NOT a
    /// struct the collector indexes, and a trait bound is not a forgeable
    /// raw-surface input. The sealed `ResolvedSurfaceAccess` is the load-bearing
    /// case: its supertrait seal is file-private (`E0603` for any out-of-file
    /// impl; the two impls are pinned by
    /// `resolved_surface_access_impls_are_exactly_the_two_tokens`), so an `impl
    /// ResolvedSurfaceAccess` value cannot be forged. `StoreView` is a plain
    /// resolution-context trait bound. The slice is the approved trait home(s) a
    /// QUALIFIED `&dyn crate::resolver_core::ResolverContext` may be written
    /// under; a one-segment `&impl T` bound is approved iff no same-name struct is
    /// collected (a trait is never a collected struct def).
    SealedTraitBound(&'static [&'static str]),
    /// A generic / associated-type / std name surfaced by a `HashMap`/`BTreeMap`
    /// / `std::mem` param context (`Key` / `Value` / `Discriminant`) — not a DTO.
    /// A one-segment bare name (`T` / `Key` / `Value`) is a benign type-param /
    /// assoc name; a QUALIFIED form is approved only under the listed std/core
    /// prefixes (`std::mem::Discriminant`, `Self::Key`).
    GenericOrAssocOrStd(&'static [&'static str]),
    /// A non-collected EXTERNAL value type (a handle / context / config / snapshot
    /// / span / diagnostic / selector whose def home is deliberately NOT a read
    /// root) carrying no resolution authority. A QUALIFIED ref is approved only
    /// under the listed external home(s); an UNQUALIFIED ref is approved iff no
    /// same-name def is collected in the read roots (else the real def classifies
    /// it — fail closed on the ambiguity).
    ExternalNonAuthority(&'static [&'static str]),
}

impl NonAuthorityCategory {
    /// The approved qualified module-path prefixes this category permits.
    fn approved_homes(&self) -> &'static [&'static str] {
        match self {
            NonAuthorityCategory::SealedTraitBound(h)
            | NonAuthorityCategory::GenericOrAssocOrStd(h)
            | NonAuthorityCategory::ExternalNonAuthority(h) => h,
        }
    }
}

/// Whether a written QUALIFIER (relative `crate` / `self` / `super` segments
/// rebased onto `from_module`) is a SUFFIX of (or equal to) ANY approved home
/// (`::`-split), OR exactly equals one. `Self::` is treated as a benign
/// assoc-qualifier (a `Self::Discriminant` / `Self::Key` associated type),
/// matched when `Self` is an approved home token.
fn qualifier_matches_approved_home(
    qualifier: &[String],
    from_module: &str,
    homes: &[&str],
) -> bool {
    // `Self::<assoc>` — an associated type on the impl's own type; benign when the
    // category lists `Self` as an approved home token.
    if qualifier.first().map(String::as_str) == Some("Self") {
        return homes.contains(&"Self");
    }
    let normalized = normalize_relative_qualifier(qualifier, from_module);
    homes.iter().any(|home| {
        let home_segs: Vec<String> = home.split("::").map(str::to_string).collect();
        // The candidate "module" is the approved home; the written qualifier must
        // be a SUFFIX of (or equal to) it (re-export / partial-path slack, the same
        // suffix-or-equal relation `module_qualifier_matches` uses for a def
        // module).
        normalized
            .as_deref()
            .is_some_and(|q| module_qualifier_matches(home, q))
            || module_qualifier_matches(home, qualifier)
            // ...or the written qualifier IS the home exactly (an extern-crate path
            // like `verter_span`), spelled explicitly (subsumed by the suffix-or-
            // equal checks above, kept for clarity).
            || home_segs == qualifier
    })
}

/// THE qualifier-aware CATEGORY exemption rule (F2): whether an UNRESOLVED
/// reference `tref` is exempted by the non-field-bearing `category` — NEVER by
/// bare final segment. The ref's `path` + `from_module` and the collision index
/// decide:
///   - QUALIFIED (`>=2` segments): exempt ONLY if the written qualifier denotes an
///     approved home for the category (`evil::Span` does NOT match the `Span`
///     external home; `crate::resolver_core::ResolverContext` does).
///   - UNQUALIFIED (one segment): a `GenericOrAssocOrStd` bare name (`T` / `Key`)
///     is always benign; a `SealedTraitBound` / `ExternalNonAuthority` bare name
///     is exempt ONLY when NO same-name def is collected (a collected same-name
///     def must classify it — fail closed on the ambiguity).
fn non_authority_category_exempts(
    tref: &TypeRef,
    category: NonAuthorityCategory,
    name_to_ids: &NameDefIndex,
) -> bool {
    let TypeRef::Unresolved {
        from_module,
        path,
        final_segment,
    } = tref
    else {
        return false; // a resolved ref is classified by its id, never a category
    };
    let has_collision = name_to_ids
        .get(final_segment)
        .is_some_and(|s| !s.is_empty());
    if path.len() >= 2 {
        let qualifier = &path[..path.len() - 1];
        return qualifier_matches_approved_home(qualifier, from_module, category.approved_homes());
    }
    // Unqualified one-segment ref.
    match category {
        // A bare type-param / associated-type / std name (`T` / `Key` / `Value`)
        // is structurally not a forgeable DTO reference.
        NonAuthorityCategory::GenericOrAssocOrStd(_) => true,
        // A bare trait bound / external value type is benign ONLY when no
        // collected same-name def exists — a collected same-name forgeable def
        // must be classified by the resolver, never blessed by this bare-name
        // category.
        NonAuthorityCategory::SealedTraitBound(_)
        | NonAuthorityCategory::ExternalNonAuthority(_) => !has_collision,
    }
}

/// A KNOWN non-authority input ident (GAP-6 / §G): a PascalCase param type that
/// appears on a TypeExpr-bearing sink boundary and is NOT forgeable raw
/// authority. Each entry is EITHER a QUALIFIED `(module, name)` that MUST be
/// defined in the read type-def roots (anti-vacuity — a stale exemption FAILS,
/// surfaced by `non_authority_input_anti_vacuity_violations`), OR an explicit
/// non-field-bearing CATEGORY (a sealed trait bound / generic-or-assoc name /
/// non-collected external) with a justification and no def requirement.
///
/// A QUALIFIED entry whose home is read RESOLVES when referenced (so it is
/// classified by the resolver, never reaching this fallback) — it is listed for
/// the anti-vacuity rail. A CATEGORY entry is the actual fallback the input
/// completeness check matches an UNRESOLVED ref's final segment against.
#[derive(Debug, Clone, Copy)]
enum KnownNonAuthorityInput {
    /// `(module, name)` — anti-vacuity-checked: must exist in the read roots.
    Qualified(&'static str, &'static str),
    /// A non-field-bearing category name with no def requirement.
    Category(&'static str, NonAuthorityCategory),
}

impl KnownNonAuthorityInput {
    fn name(&self) -> &'static str {
        match self {
            KnownNonAuthorityInput::Qualified(_, n) => n,
            KnownNonAuthorityInput::Category(n, _) => n,
        }
    }

    /// The non-field-bearing CATEGORY of a `Category` entry, if any (a `Qualified`
    /// entry is justified by its `(module, name)` def, not a category).
    fn category(&self) -> Option<NonAuthorityCategory> {
        match self {
            KnownNonAuthorityInput::Category(_, c) => Some(*c),
            KnownNonAuthorityInput::Qualified(..) => None,
        }
    }
}

const KNOWN_NON_AUTHORITY_INPUT_IDENTS: &[KnownNonAuthorityInput] = &[
    // --- Host / engine / session handles (own the resolver; not a forgeable
    // surface — they ARE the authority that resolves). Collected in read roots,
    // so anti-vacuity-checked. ---
    KnownNonAuthorityInput::Qualified("crate", "VerterHost"),
    KnownNonAuthorityInput::Qualified(
        "crate::resolver_core::component_meta_query_engine",
        "ComponentMetaQueryEngine",
    ),
    KnownNonAuthorityInput::Qualified(
        "crate::project_semantic_dispatch",
        "ProjectSemanticDispatch",
    ),
    // --- Context / cursor / scope / env carriers (resolution context). Collected
    // ones are anti-vacuity-checked; the non-collected externals are categorized. ---
    KnownNonAuthorityInput::Qualified("crate::framework::ctx", "FrameworkAdapterCtx"),
    // `StoreView` is a `pub trait` (`crate::resolver_core::StoreView`), taken as a
    // `&impl StoreView` bound — NOT a struct the collector indexes, and a trait
    // bound is not a forgeable raw-surface input. Approved trait home so a
    // QUALIFIED `crate::resolver_core::StoreView` matches; a bare `&impl StoreView`
    // matches iff no same-name struct is collected.
    KnownNonAuthorityInput::Category(
        "StoreView",
        NonAuthorityCategory::SealedTraitBound(&["crate::resolver_core"]),
    ),
    // `ProjectionDispatch` / `ResolverContext` / `ExecutorResolveCtx` /
    // `RequestStoreView` are external / sealed-trait resolution contexts.
    // `ResolverContext` is a sealed `pub trait crate::resolver_core::
    // ResolverContext` (written `crate::resolver_core::ResolverContext` /
    // `super::super::ResolverContext`); the others have no read-root struct def
    // (or resolve when collected) so only the unqualified-no-collision arm applies.
    KnownNonAuthorityInput::Category(
        "ProjectionDispatch",
        NonAuthorityCategory::ExternalNonAuthority(&[]),
    ),
    KnownNonAuthorityInput::Category(
        "ResolverContext",
        NonAuthorityCategory::SealedTraitBound(&["crate::resolver_core"]),
    ),
    KnownNonAuthorityInput::Category(
        "ExecutorResolveCtx",
        NonAuthorityCategory::ExternalNonAuthority(&[]),
    ),
    KnownNonAuthorityInput::Category(
        "RequestStoreView",
        NonAuthorityCategory::ExternalNonAuthority(&[]),
    ),
    // `EvalEnv` lives in `verter_semantic::analysis::type_eval` (NOT a read root).
    KnownNonAuthorityInput::Category(
        "EvalEnv",
        NonAuthorityCategory::ExternalNonAuthority(&["verter_semantic::analysis::type_eval"]),
    ),
    // --- Snapshots / analysis inputs (already-analyzed parse facts). ---
    KnownNonAuthorityInput::Qualified("crate::types", "FileAnalysisSnapshot"),
    KnownNonAuthorityInput::Qualified(
        "crate::resolver_core::shallow_file_state",
        "ShallowFileState",
    ),
    KnownNonAuthorityInput::Category(
        "AnalyzedComponent",
        NonAuthorityCategory::ExternalNonAuthority(&["verter_semantic::analysis"]),
    ),
    // --- Macro / analyzed producer facts (the macro path lowers them at its
    // producer boundary). `verter_semantic::analysis::types` IS a read root. ---
    KnownNonAuthorityInput::Qualified("verter_semantic::analysis::types", "AnalyzedMacro"),
    KnownNonAuthorityInput::Qualified("verter_semantic::analysis::types", "AnalyzedPropField"),
    KnownNonAuthorityInput::Qualified("verter_semantic::analysis::types", "AnalyzedEmitField"),
    KnownNonAuthorityInput::Qualified("verter_semantic::analysis::types", "AnalyzedSlotField"),
    KnownNonAuthorityInput::Qualified("verter_semantic::analysis::types", "AnalyzedExposeField"),
    // --- Config / identity / scope value types (no resolution authority). ---
    KnownNonAuthorityInput::Qualified("crate::types", "HostConfig"),
    KnownNonAuthorityInput::Category(
        "IdeProjectConfig",
        NonAuthorityCategory::ExternalNonAuthority(&[
            "verter_semantic::analysis::project_resolver",
        ]),
    ),
    KnownNonAuthorityInput::Qualified("crate::semantic_query", "DeclIdentity"),
    KnownNonAuthorityInput::Qualified("crate::semantic_query", "ResolvedDeclSlotIdentity"),
    // `DeclarationId` lives in `verter_semantic::analysis::type_eval` (unread).
    KnownNonAuthorityInput::Category(
        "DeclarationId",
        NonAuthorityCategory::ExternalNonAuthority(&["verter_semantic::analysis::type_eval"]),
    ),
    KnownNonAuthorityInput::Qualified("crate::semantic_query", "SemanticQueryKey"),
    KnownNonAuthorityInput::Qualified("crate::component_meta_caches", "ShapeCacheKey"),
    KnownNonAuthorityInput::Qualified("crate::component_meta_caches", "ShapeSubject"),
    KnownNonAuthorityInput::Qualified("crate::component_meta_caches", "ShapeDemand"),
    // --- Framework selector / plan / resolved-plan inputs (typed selector / plan
    // data the executor resolves THROUGH the shared engine; a `ResolvedSurfaces`
    // / `ResolvedDemand` / `PlannedDemand` carries surfaces only transitively and
    // is normalized by an allowlisted sink). `…framework_surface::plan` IS a read
    // root (so these resolve and are anti-vacuity-checked rather than trusted). ---
    KnownNonAuthorityInput::Category(
        "FrameworkSurfaceSelector",
        NonAuthorityCategory::ExternalNonAuthority(&["crate::typeinfo::framework_surface"]),
    ),
    KnownNonAuthorityInput::Qualified(
        "crate::typeinfo::framework_surface::plan",
        "ResolvedComponentSelector",
    ),
    KnownNonAuthorityInput::Qualified(
        "crate::typeinfo::framework_surface::plan",
        "ResolvedSurfaces",
    ),
    KnownNonAuthorityInput::Qualified("crate::typeinfo::framework_surface::plan", "ResolvedDemand"),
    KnownNonAuthorityInput::Qualified("crate::typeinfo::framework_surface::plan", "PlannedDemand"),
    // --- Span / diagnostic / misc non-authority value types whose def home is
    // not a read root. `Span` is `verter_span::Span` (a byte range). ---
    KnownNonAuthorityInput::Category(
        "Span",
        NonAuthorityCategory::ExternalNonAuthority(&["verter_span"]),
    ),
    KnownNonAuthorityInput::Qualified("crate::semantic_query", "ProjectionMode"),
    KnownNonAuthorityInput::Category(
        "PublishedSurfaceKind",
        NonAuthorityCategory::ExternalNonAuthority(&["crate::meta_resolve::projection_demand"]),
    ),
    // --- Parse-domain macro diagnostics / kind (producer facts; their home
    // `verter_semantic::analysis::component_meta` is NOT a read root). ---
    KnownNonAuthorityInput::Category(
        "MacroExpansionDiagnostics",
        NonAuthorityCategory::ExternalNonAuthority(&["verter_semantic::analysis::component_meta"]),
    ),
    KnownNonAuthorityInput::Category(
        "MacroExpansionKind",
        NonAuthorityCategory::ExternalNonAuthority(&["verter_semantic::analysis::component_meta"]),
    ),
    // --- std / assoc-type generic idents (`Key` / `Value` assoc names,
    // `Discriminant` from `std::mem`). Not a DTO, not authority. A bare `Key` /
    // `Value` is a one-segment assoc/type-param name; a `Self::Key` / `Self::Value`
    // / `std::mem::Discriminant` qualified form is approved under `Self` / `std`. ---
    KnownNonAuthorityInput::Category("Key", NonAuthorityCategory::GenericOrAssocOrStd(&["Self"])),
    KnownNonAuthorityInput::Category(
        "Value",
        NonAuthorityCategory::GenericOrAssocOrStd(&["Self"]),
    ),
    KnownNonAuthorityInput::Category(
        "Discriminant",
        NonAuthorityCategory::GenericOrAssocOrStd(&["std::mem", "core::mem", "Self"]),
    ),
    // --- The SEALED `ResolvedSurfaceAccess` trait bound: `&impl
    // ResolvedSurfaceAccess` surfaces as this ident. It is NOT a forgeable input —
    // the supertrait seal is module-private (`E0603` for any out-of-file impl),
    // implemented STRUCTURALLY ONLY for the two real sealed tokens, so an `impl
    // ResolvedSurfaceAccess` value cannot be forged. Approved trait home so a
    // QUALIFIED `…::resolved_surface_access::ResolvedSurfaceAccess` matches. ---
    KnownNonAuthorityInput::Category(
        "ResolvedSurfaceAccess",
        NonAuthorityCategory::SealedTraitBound(&[
            "crate::typeinfo::framework_surface::resolved_surface_access",
        ]),
    ),
];

/// FAIL-CLOSED ANTI-VACUITY (§G): every QUALIFIED non-authority input entry's
/// `(module, name)` MUST resolve to a collected def in the read roots. A stale
/// exemption (a renamed / moved / deleted type that a `Qualified` entry still
/// names) FAILS loudly rather than silently trusting a name that no longer
/// exists. CATEGORY entries (sealed trait bound / generic-or-assoc / non-collected
/// external) carry no def requirement — they are justified by WHAT they are.
fn non_authority_input_anti_vacuity_violations(name_to_ids: &NameDefIndex) -> Vec<String> {
    let empty = std::collections::BTreeSet::new();
    let mut out = Vec::new();
    for entry in KNOWN_NON_AUTHORITY_INPUT_IDENTS {
        if let KnownNonAuthorityInput::Qualified(module, name) = entry {
            let ids = name_to_ids.get(*name).unwrap_or(&empty);
            if !ids.contains(&TypeDefId::new(*module, *name)) {
                let observed: Vec<&str> = ids.iter().map(|id| id.module.as_str()).collect();
                out.push(format!(
                    "qualified non-authority input `{module}::{name}` is NOT defined in its named \
                     module (observed in {observed:?}) — a stale exemption; either fix the \
                     `(module, name)` to its real home, recategorize it as a non-field-bearing \
                     category, or remove the exemption"
                ));
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// The non-field-bearing CATEGORY of a known non-authority input NAME, if any
/// (`Category` entries only — a `Qualified` entry is the anti-vacuity rail and
/// never blesses an UNRESOLVED ref through this fallback).
fn non_authority_input_category(name: &str) -> Option<NonAuthorityCategory> {
    KNOWN_NON_AUTHORITY_INPUT_IDENTS
        .iter()
        .find(|e| e.name() == name)
        .and_then(KnownNonAuthorityInput::category)
}

/// FAIL-CLOSED INPUT completeness (GAP-6 — the input-side analogue of
/// [`unclassifiable_output_idents`]): for every reachable (non-test,
/// non-module-private) sink fn whose OUTPUT (or `&mut` out-param) is
/// TypeExpr-bearing, EVERY PascalCase INPUT type ident must be CLASSIFIABLE — in
/// `defs` (the forgeable closure could decide its authority), a known container
/// ([`KNOWN_CONTAINER_OUTPUT_IDENTS`]), a known safe token / construction-chain
/// struct (so a token param is recognized), or a known non-authority external
/// ([`KNOWN_NON_AUTHORITY_INPUT_IDENTS`]). An UNKNOWN PascalCase input type on a
/// bearing boundary FAILS: it could be a FORGEABLE wrapper whose def home is
/// unread, which the cross-sink check would then silently miss (it only fires on
/// a KNOWN-forgeable input). This makes the INPUT side fail-closed and symmetric
/// with the output side.
///
/// The non-authority exemption is QUALIFIER-AWARE (F2): an UNRESOLVED ref is
/// exempted by a `Category` entry only when the ref's PATH agrees with the
/// category's approved homes ([`non_authority_category_exempts`]) — NEVER by bare
/// final segment, so a forged `evil::Span` fires and a same-name COLLECTED def is
/// never blessed by a bare-name exemption.
fn unclassifiable_input_idents(
    sigs: &[SinkFnSig],
    typeexpr_bearing: &std::collections::BTreeSet<TypeDefId>,
    name_to_ids: &NameDefIndex,
) -> Vec<(String, String)> {
    let container: std::collections::BTreeSet<&str> =
        KNOWN_CONTAINER_OUTPUT_IDENTS.iter().copied().collect();
    let mut out = Vec::new();
    for sig in sigs {
        if sig.test_gated || sig.module_private {
            continue;
        }
        // Only fns whose OUTPUT channel is TypeExpr-bearing are a publication
        // boundary the input side must be complete for.
        let output_bearing = sig
            .output_idents
            .iter()
            .chain(sig.mut_outparam_idents.iter())
            .any(|tref| {
                tref.resolved()
                    .is_some_and(|id| typeexpr_bearing.contains(id))
            });
        if !output_bearing {
            continue;
        }
        for tref in &sig.input_idents {
            // A RESOLVED ref is classifiable — a collected def (the forgeable
            // closure decides its authority), a seed, OR a safe-token / chain
            // struct (all of which are read and resolve concretely).
            if tref.resolved().is_some() {
                continue;
            }
            let id = tref.final_segment();
            let mut chars = id.chars();
            let Some(first) = chars.next() else {
                continue;
            };
            if !first.is_ascii_uppercase() || id.len() < 2 {
                continue;
            }
            // An UNRESOLVED ref is benign only if it is a known container, OR a
            // justified non-authority external whose written PATH agrees with the
            // category's approved homes (QUALIFIER-AWARE — a forged `evil::Span`
            // does NOT match, a same-name COLLECTED def is not blessed). Anything
            // else is fail-closed (a forgeable wrapper whose def home is unread, or
            // an ambiguous / forged-qualifier bare name).
            if container.contains(id) {
                continue;
            }
            if let Some(category) = non_authority_input_category(id) {
                if non_authority_category_exempts(tref, category, name_to_ids) {
                    continue;
                }
            }
            out.push((format!("{}::{}", sig.module_path, sig.name), id.to_string()));
        }
    }
    out.sort();
    out.dedup();
    out
}

#[test]
fn cross_sink_raw_authority_to_type_expr_boundary() {
    // The field-closure is computed from the real type defs; the scan is over
    // the real registered PUBLICATION sink scopes. With the admitted-token chain
    // in place every reachable forgeable-input → TypeExpr boundary AT THOSE
    // Kind-A publication sinks is either routed through a token (input is no
    // longer a forgeable seed) or on the closed sink-local allowlist. The Kind-B
    // raise-then-decide residual (`legacy_semantic_type_expr_bridge` route) is
    // deferred to 8-A4 and pinned separately (see the section header) — it is
    // not a publication sink and is intentionally outside this scan.
    let (defs, name_to_ids) = collect_type_defs();
    let bearing = typeexpr_bearing_closure(&defs);
    let forgeable = forgeable_authority_closure(&defs, &bearing);
    // FAIL-CLOSED anti-vacuity: every qualified safe-input / construction-chain
    // token must be defined at its sanctioned `(module, name)` — a renamed/moved
    // token FAILS. Under module-qualified identity a shared bare name is two
    // DISTINCT ids (no "accepted because bearing" carve-out); an ambiguous
    // UNQUALIFIED reference to such a name is caught fail-closed at the boundary.
    let collisions = qualified_safe_input_collision_violations(&name_to_ids);
    assert!(
        collisions.is_empty(),
        "qualified safe-input identity violation(s) — a safe-input / construction-chain token name \
         is missing or moved (module-qualified `(module, name)` identity must resolve to its \
         sanctioned home):\n{}",
        collisions.join("\n")
    );
    // §G FAIL-CLOSED ANTI-VACUITY: every QUALIFIED non-authority input exemption
    // must still resolve to a collected def — a stale exemption FAILS loudly.
    let stale_non_authority = non_authority_input_anti_vacuity_violations(&name_to_ids);
    assert!(
        stale_non_authority.is_empty(),
        "qualified non-authority input anti-vacuity violation(s) — a `Qualified` non-authority \
         exemption names a type that is no longer defined in its module (recategorize, re-home, or \
         remove):\n{}",
        stale_non_authority.join("\n")
    );
    // CRUX: the two `IndexSignature` defs are DISTINCT module-qualified ids — the
    // authority `crate::semantic_query::IndexSignature` (SemanticNodeId fields)
    // and the bearing `verter_type_expr::IndexSignature` (TypeExpr fields) — NOT
    // one merged bare-name slot.
    let sem_index = TypeDefId::new("crate::semantic_query", "IndexSignature");
    let te_index = TypeDefId::new("verter_type_expr", "IndexSignature");
    assert!(
        defs.contains_key(&sem_index) && defs.contains_key(&te_index),
        "anti-vacuity: BOTH `IndexSignature` homes must be collected as DISTINCT ids \
         (the qualified-identity crux); got sem={}, te={}",
        defs.contains_key(&sem_index),
        defs.contains_key(&te_index)
    );
    assert!(
        forgeable.contains(&sem_index) && !forgeable.contains(&te_index),
        "the authority `crate::semantic_query::IndexSignature` MUST be forgeable raw-authority and \
         the bearing `verter_type_expr::IndexSignature` MUST NOT (it is an already-lowered-IR \
         output-side holder) — bare-name merging would conflate them"
    );
    assert!(
        bearing.contains(&te_index) && !bearing.contains(&sem_index),
        "the type-expr `IndexSignature` is bearing; the semantic-query one (SemanticNodeId fields) \
         is NOT bearing — distinct ids classify on their own merits"
    );
    // Sanity: the closure actually flagged the cross-crate published DTOs (a
    // closure that reached NOTHING would vacuously pass).
    for (module, name) in [
        ("verter_type_expr", "TypeExpr"),
        (
            "verter_semantic::analysis::type_expand::request",
            "ExpandedField",
        ),
        (
            "verter_semantic::analysis::type_solver::query_engine",
            "ProjectedSurface",
        ),
        (
            "crate::typeinfo::framework_surface::results",
            "MacroSurfaceDtos",
        ),
    ] {
        let id = TypeDefId::new(module, name);
        assert!(
            bearing.contains(&id),
            "field-closure must flag `{id}` as TypeExpr-bearing (closure regressed / seed home \
             unparsed)"
        );
    }
    // Sanity: the INPUT closure flagged the forgeable seeds, and the admitted
    // tokens are EXCLUDED (a token is the allowed input, never raw authority —
    // even though it carries a `SurfaceMember` field internally).
    for (module, name) in [
        ("crate::semantic_query", "SemanticNodeId"),
        ("crate::semantic_query", "SurfaceMember"),
        (
            "crate::typeinfo::framework_surface::vue_exec",
            "VueMacroSurface",
        ),
    ] {
        let id = TypeDefId::new(module, name);
        assert!(
            forgeable.contains(&id),
            "input field-closure must flag `{id}` as forgeable authority (closure regressed)"
        );
    }
    // The POLICY-ADMITTED safe inputs are EXCLUDED from the forgeable closure —
    // a fn taking only one of these never fires (they are the sanctioned admitted
    // input, never raw authority, even though they wrap a forgeable seed).
    for (module, tok) in POLICY_ADMITTED_SAFE_INPUTS {
        let id = TypeDefId::new(*module, *tok);
        assert!(
            !forgeable.contains(&id),
            "policy-admitted safe input `{id}` MUST be EXCLUDED from the forgeable-authority set — \
             it is the sanctioned admitted input, never raw authority (even though it wraps a \
             forgeable seed)"
        );
    }
    // The PRE-admission construction-chain structs are likewise NOT members of
    // the forgeable CLOSURE (so they don't poison every wrapper) — but a sink fn
    // taking one DIRECTLY still fires via the cross-sink direct chain-struct
    // check (asserted in the self-test). They must not be in the closure set.
    for (module, chain) in SEALED_CONSTRUCTION_CHAIN_STRUCTS {
        let id = TypeDefId::new(*module, *chain);
        assert!(
            !forgeable.contains(&id),
            "construction-chain struct `{id}` MUST NOT be a member of the forgeable CLOSURE set \
             (it fires via the DIRECT chain-struct check, not closure propagation — keeping it out \
             of the closure prevents every wrapper of it from being marked forgeable)"
        );
    }
    let sigs = collect_sink_fn_sigs(&name_to_ids);
    assert!(
        !sigs.is_empty(),
        "expected to collect production fns from the registered sink scopes; found none — the \
         scanner or scope prefixes regressed"
    );
    // FAIL-CLOSED completeness: every OUTPUT ident a sink fn returns must be
    // CLASSIFIABLE — RESOLVED to a `TypeDefId` (so the field-closure could decide
    // bearing-ness) OR a known std/primitive/container/non-DTO-external ident. An
    // UNRESOLVED PascalCase output ident is a TypeExpr-bearing WRAPPER DTO defined
    // OUTSIDE the read file set treated as a non-bearing leaf (or an ambiguous
    // bare name) — the silent under-classification this fails loudly on.
    let unclassifiable = unclassifiable_output_idents(&sigs, &name_to_ids);
    assert!(
        unclassifiable.is_empty(),
        "STRUCTURAL closure completeness violation(s) — a sink fn returns a PascalCase output type \
         the field-closure CANNOT classify (not RESOLVED to a read def, not a known std/container/\
         non-DTO ident). If it is a TypeExpr-bearing WRAPPER DTO, its def home is unread and the \
         guard would silently under-classify it; widen `type_def_source_files` EXTERNAL roots to \
         cover its crate, or add it to `KNOWN_NON_DTO_OUTPUT_IDENTS` if it is a genuinely \
         non-bearing external type:\n{}",
        unclassifiable
            .iter()
            .map(|(f, id)| format!("  `{f}` returns unclassifiable `{id}`"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    // GAP-6: FAIL-CLOSED INPUT completeness — symmetric with the output side.
    // Every PascalCase INPUT type on a TypeExpr-bearing sink boundary must be
    // CLASSIFIABLE (RESOLVED to a read def / seed / token, a known container, or a
    // known non-authority external). An UNRESOLVED input type could be a forgeable
    // wrapper whose def home is unread — which the cross-sink check would then
    // silently miss — so it FAILS loudly.
    let unclassifiable_inputs = unclassifiable_input_idents(&sigs, &bearing, &name_to_ids);
    assert!(
        unclassifiable_inputs.is_empty(),
        "STRUCTURAL input completeness violation(s) — a sink fn with a `TypeExpr`-bearing output \
         takes a PascalCase INPUT type the scanner CANNOT classify (not in the read `defs`, not a \
         known container, not a safe token / construction-chain struct, not a known non-authority \
         external). If it is a FORGEABLE wrapper its def home is unread and the cross-sink check \
         would silently miss it; widen `type_def_source_files` EXTERNAL roots to cover its crate, \
         or add it to `KNOWN_NON_AUTHORITY_INPUT_IDENTS` if it is a genuinely non-authority \
         external type (with a justification):\n{}",
        unclassifiable_inputs
            .iter()
            .map(|(f, id)| format!("  `{f}` takes unclassifiable input `{id}`"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    let violations = cross_sink_raw_authority_violations(&sigs, &bearing, &forgeable);
    assert!(
        violations.is_empty(),
        "STRUCTURAL cross-sink raw-authority → `TypeExpr` boundary violation(s) — a reachable \
         production fn pairs a forgeable surface/member/node input (directly or through a wrapper \
         type) with a `TypeExpr`-bearing output without routing through the admitted-token \
         chain:\n{}",
        violations.join("\n")
    );
}

#[test]
fn unclassifiable_output_idents_self_test_discriminates() {
    // EMPTY fixture index: the only resolvable names are seeds (`ExpandedField`)
    // via the seed merge; an unread wrapper stays `Unresolved`.
    let fixture: NameDefIndex = BTreeMap::new();
    let mk = |name: &str, outputs: &[&str], mut_out: &[&str], module_private: bool| -> SinkFnSig {
        let sig = synthetic_sig(
            &fixture,
            "crate::meta_resolve::projectors::output_sink",
            name,
            &[],
            outputs,
            mut_out,
        );
        if module_private {
            sig.marked_module_private()
        } else {
            sig
        }
    };

    // GREEN: a fn returning a KNOWN DTO (`ExpandedField`, a seed) wrapped in a
    // known container (`Vec`), plus a primitive + a generic param — all
    // classifiable.
    let green = mk("ok", &["Vec", "ExpandedField", "bool", "T"], &[], false);
    assert!(
        unclassifiable_output_idents(&[green], &fixture).is_empty(),
        "self-test: a fn returning only classifiable idents (resolved DTO + container + primitive + \
         generic param) MUST pass; got: {:?}",
        unclassifiable_output_idents(
            &[mk("ok", &["Vec", "ExpandedField", "bool", "T"], &[], false)],
            &fixture
        )
    );

    // GREEN: a known non-DTO external written under its APPROVED qualified home
    // (`verter_span::Span`) is classifiable; the qualifier-aware exemption matches.
    let qualified_external = synthetic_sig(
        &fixture,
        "crate::meta_resolve::projectors::output_sink",
        "ok_qualified_external",
        &[],
        &[],
        &[],
    );
    let qualified_external = SinkFnSig {
        output_idents: [resolve_type_ref(
            &["verter_span".to_string(), "Span".to_string()],
            "crate::meta_resolve::projectors::output_sink",
            &UseIndex::default(),
            &name_index_with_seed_ids(&fixture),
            &ReExportIndex::default(),
            &UseBindingIndex::default(),
        )]
        .into_iter()
        .collect(),
        ..qualified_external
    };
    assert!(
        unclassifiable_output_idents(&[qualified_external], &fixture).is_empty(),
        "self-test: a known non-DTO external written under its approved qualified home \
         (`verter_span::Span`) MUST be classifiable (qualifier-aware exemption)"
    );

    // RED: a fn returning an UNREAD PascalCase wrapper DTO (`UnreadWrapperDto`)
    // that stays UNRESOLVED and is NOT a container / known-non-DTO — exactly the
    // under-classified bearing-wrapper-defined-in-an-unread-crate case the
    // fail-closed check catches.
    let red = mk("leak", &["Option", "UnreadWrapperDto"], &[], false);
    let v = unclassifiable_output_idents(&[red], &fixture);
    assert!(
        v.iter().any(|(f, id)| f.contains("leak") && id == "UnreadWrapperDto"),
        "self-test: an UNREAD PascalCase output DTO (`UnreadWrapperDto`) MUST FIRE the fail-closed \
         completeness check; got: {v:?}"
    );

    // RED (F2 — forged qualifier): a known non-DTO name written under a FORGED
    // qualifier (`evil::Span`) must NOT be blessed by the bare `Span` exemption —
    // the qualifier does not match the approved `verter_span` home, so it FIRES.
    let forged_external = SinkFnSig {
        module_path: "crate::meta_resolve::projectors::output_sink".to_string(),
        name: "leak_forged_external".to_string(),
        input_idents: std::collections::BTreeSet::new(),
        output_idents: [resolve_type_ref(
            &["evil".to_string(), "Span".to_string()],
            "crate::meta_resolve::projectors::output_sink",
            &UseIndex::default(),
            &name_index_with_seed_ids(&fixture),
            &ReExportIndex::default(),
            &UseBindingIndex::default(),
        )]
        .into_iter()
        .collect(),
        mut_outparam_idents: std::collections::BTreeSet::new(),
        test_gated: false,
        module_private: false,
    };
    let v = unclassifiable_output_idents(&[forged_external], &fixture);
    assert!(
        v.iter()
            .any(|(f, id)| f.contains("leak_forged_external") && id == "Span"),
        "self-test (F2): a known non-DTO name (`Span`) written under a FORGED qualifier \
         (`evil::Span`) MUST FIRE the output completeness check (not blessed by the bare-name \
         exemption); got: {v:?}"
    );

    // RED: an unread wrapper in a `&mut` OUT-param channel also fires.
    let red_outparam = mk("mutate", &[], &["UnreadOutParamDto"], false);
    let v = unclassifiable_output_idents(&[red_outparam], &fixture);
    assert!(
        v.iter().any(|(_, id)| id == "UnreadOutParamDto"),
        "self-test: an UNREAD `&mut` out-param DTO MUST FIRE the completeness check; got: {v:?}"
    );

    // A MODULE-PRIVATE fn is DROPPED (it is sink-confined; its outputs do not
    // cross the boundary).
    let private = mk("private_leak", &["UnreadWrapperDto"], &[], true);
    assert!(
        unclassifiable_output_idents(&[private], &fixture).is_empty(),
        "self-test: a module-private fn's outputs MUST be dropped from the completeness check"
    );
}

#[test]
fn unclassifiable_input_idents_self_test_discriminates() {
    // Fixture index: `ExpandedField` resolves (seed → bearing); the collected
    // `VerterHost` (a `Qualified` non-authority entry, collected in the real tree)
    // is present so it resolves; an unread input wrapper stays `Unresolved`; a
    // CATEGORY external stays `Unresolved` and is classified by the QUALIFIER-AWARE
    // category rule (F2), never by bare name.
    let mut fixture: NameDefIndex = BTreeMap::new();
    fixture
        .entry("VerterHost".to_string())
        .or_default()
        .insert(TypeDefId::new("crate", "VerterHost"));
    let bearing = typeexpr_bearing_closure(&synthetic_defs(&[]).0);

    let mk = |name: &str, inputs: &[&str], outputs: &[&str], module_private: bool| -> SinkFnSig {
        let sig = synthetic_sig(
            &fixture,
            "crate::meta_resolve::projectors::output_sink",
            name,
            inputs,
            outputs,
            &[],
        );
        if module_private {
            sig.marked_module_private()
        } else {
            sig
        }
    };
    // Build a sink fn whose single input is a directly-written PATH (so the F2
    // qualified / forged-qualifier cases can be exercised), with a bearing output.
    let mk_path_input = |fn_name: &str, input_path: &[&str]| -> SinkFnSig {
        let module = "crate::meta_resolve::projectors::output_sink";
        SinkFnSig {
            module_path: module.to_string(),
            name: fn_name.to_string(),
            input_idents: [resolve_type_ref(
                &input_path.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
                module,
                &UseIndex::default(),
                &name_index_with_seed_ids(&fixture),
                &ReExportIndex::default(),
                &UseBindingIndex::default(),
            )]
            .into_iter()
            .collect(),
            output_idents: [TypeRef::Resolved(synthetic_id("ExpandedField"))]
                .into_iter()
                .collect(),
            mut_outparam_idents: std::collections::BTreeSet::new(),
            test_gated: false,
            module_private: false,
        }
    };

    // RED: a fn with a TypeExpr-bearing output (`ExpandedField`) takes an UNKNOWN
    // PascalCase input (`UnreadInputWrapperDto`) that stays UNRESOLVED — not a
    // container / safe-token / non-authority. It could be a forgeable wrapper
    // whose def home is unread; the cross-sink check would silently miss it. MUST
    // FIRE.
    let red = mk(
        "leak_input",
        &["UnreadInputWrapperDto"],
        &["ExpandedField"],
        false,
    );
    let v = unclassifiable_input_idents(&[red], &bearing, &fixture);
    assert!(
        v.iter()
            .any(|(f, id)| f.contains("leak_input") && id == "UnreadInputWrapperDto"),
        "self-test: an UNKNOWN PascalCase INPUT on a TypeExpr-bearing boundary MUST FIRE the input \
         completeness check; got: {v:?}"
    );

    // RED (F2 — forged qualifier on a CATEGORY external): an unresolved
    // `evil::Span` input must NOT be blessed by the bare `Span` category — the
    // forged qualifier does not match the approved `verter_span` home, so it FIRES.
    let forged_span = mk_path_input("leak_forged_span", &["evil", "Span"]);
    let v = unclassifiable_input_idents(&[forged_span], &bearing, &fixture);
    assert!(
        v.iter().any(|(f, id)| f.contains("leak_forged_span") && id == "Span"),
        "self-test (F2): a forged-qualifier `evil::Span` input MUST FIRE (not blessed by the bare \
         `Span` category — the qualifier does not match the approved `verter_span` home); got {v:?}"
    );

    // GREEN (F2 — approved-home qualifier): a `verter_span::Span` input written
    // under its APPROVED home is classifiable (the qualifier-aware exemption
    // matches) and does NOT fire.
    let approved_span = mk_path_input("ok_approved_span", &["verter_span", "Span"]);
    assert!(
        unclassifiable_input_idents(&[approved_span], &bearing, &fixture).is_empty(),
        "self-test (F2): a `verter_span::Span` input under its approved home MUST be classifiable \
         (qualifier-aware exemption)"
    );

    // RED (F2 — collision on an UNQUALIFIED CATEGORY external): when a same-name
    // def of an ExternalNonAuthority category IS collected, a bare `Span` input is
    // NOT blessed (the collected def must classify it) — fail closed.
    let mut collided_fixture = fixture.clone();
    collided_fixture
        .entry("Span".to_string())
        .or_default()
        .insert(TypeDefId::new("crate::evil", "Span"));
    let bare_span = SinkFnSig {
        module_path: "crate::meta_resolve::projectors::output_sink".to_string(),
        name: "leak_bare_span_with_collision".to_string(),
        // A bare `Span` written where a same-name def exists resolves to the
        // collected def (unqualified exactly-one) — so to model the AMBIGUITY the
        // category must guard, write an explicit Unresolved bare `Span`.
        input_idents: [TypeRef::Unresolved {
            from_module: "crate::meta_resolve::projectors::output_sink".to_string(),
            path: vec!["Span".to_string()],
            final_segment: "Span".to_string(),
        }]
        .into_iter()
        .collect(),
        output_idents: [TypeRef::Resolved(synthetic_id("ExpandedField"))]
            .into_iter()
            .collect(),
        mut_outparam_idents: std::collections::BTreeSet::new(),
        test_gated: false,
        module_private: false,
    };
    let v = unclassifiable_input_idents(&[bare_span], &bearing, &collided_fixture);
    assert!(
        v.iter()
            .any(|(f, id)| f.contains("leak_bare_span_with_collision") && id == "Span"),
        "self-test (F2): a bare `Span` input when a same-name def is COLLECTED must FIRE (the \
         bare-name external exemption is withheld on the ambiguity — fail closed); got {v:?}"
    );

    // GREEN: a fn whose inputs are all CLASSIFIABLE — a known DTO (`ExpandedField`
    // seed), a known container (`Vec`), a known safe token
    // (`AdmittedPublishedMember`), a collected non-authority handle (`VerterHost`,
    // now in the fixture so it resolves), plus a generic param + primitive. MUST
    // NOT fire.
    let green_inputs: &[&str] = &[
        "ExpandedField",
        "Vec",
        "AdmittedPublishedMember",
        "VerterHost",
        "T",
        "bool",
    ];
    let green = mk("ok_input", green_inputs, &["ExpandedField"], false);
    assert!(
        unclassifiable_input_idents(&[green], &bearing, &fixture).is_empty(),
        "self-test: a fn whose inputs are all classifiable (resolved DTO + container + safe token + \
         resolved non-authority handle + generic + primitive) MUST pass; got: {:?}",
        unclassifiable_input_idents(
            &[mk("ok_input", green_inputs, &["ExpandedField"], false)],
            &bearing,
            &fixture
        )
    );

    // GREEN (F2 — one-segment generic): a bare one-segment `Key` (a
    // GenericOrAssocOrStd category) on a bearing boundary stays benign.
    let bare_generic = mk_path_input("ok_bare_generic", &["Key"]);
    assert!(
        unclassifiable_input_idents(&[bare_generic], &bearing, &fixture).is_empty(),
        "self-test (F2): a bare one-segment `Key` (generic/assoc category) MUST stay benign"
    );

    // GREEN: a fn with the SAME unknown input but a NON-bearing output is NOT a
    // publication boundary — the input check does not apply to it.
    let non_bearing_out = mk("non_bearing", &["UnreadInputWrapperDto"], &["bool"], false);
    assert!(
        unclassifiable_input_idents(&[non_bearing_out], &bearing, &fixture).is_empty(),
        "self-test: a fn with a NON-bearing output is not a publication boundary; its unknown input \
         must NOT fire the input completeness check"
    );

    // GREEN: a MODULE-PRIVATE fn is dropped (sink-confined).
    let private = mk(
        "private_input_leak",
        &["UnreadInputWrapperDto"],
        &["ExpandedField"],
        true,
    );
    assert!(
        unclassifiable_input_idents(&[private], &bearing, &fixture).is_empty(),
        "self-test: a module-private fn's inputs MUST be dropped from the input completeness check"
    );
}

#[test]
fn non_authority_input_anti_vacuity_self_test_discriminates() {
    // §G: a QUALIFIED non-authority input exemption is anti-vacuity-checked
    // against the read roots; a CATEGORY entry is not.

    // GREEN: an index where every QUALIFIED entry is present at its named module
    // passes. (Build the index from the QUALIFIED entries themselves.)
    let mut present: NameDefIndex = BTreeMap::new();
    for entry in KNOWN_NON_AUTHORITY_INPUT_IDENTS {
        if let KnownNonAuthorityInput::Qualified(module, name) = entry {
            present
                .entry((*name).to_string())
                .or_default()
                .insert(TypeDefId::new(*module, *name));
        }
    }
    assert!(
        non_authority_input_anti_vacuity_violations(&present).is_empty(),
        "self-test: with every QUALIFIED non-authority entry present at its module, anti-vacuity \
         MUST pass; got {:?}",
        non_authority_input_anti_vacuity_violations(&present)
    );

    // RED: drop a QUALIFIED entry (`VerterHost`) entirely — the stale exemption
    // fires.
    let mut missing = present.clone();
    missing.remove("VerterHost");
    let v = non_authority_input_anti_vacuity_violations(&missing);
    assert!(
        v.iter().any(|m| m.contains("VerterHost")),
        "self-test: a QUALIFIED non-authority entry with NO def in the read roots MUST FIRE \
         (anti-vacuity); got {v:?}"
    );

    // RED: move a QUALIFIED entry to a WRONG module — the exact `(module, name)`
    // no longer matches, so it fires.
    let mut moved = present.clone();
    moved.insert(
        "VerterHost".to_string(),
        [TypeDefId::new("crate::some::other::place", "VerterHost")]
            .into_iter()
            .collect(),
    );
    let v = non_authority_input_anti_vacuity_violations(&moved);
    assert!(
        v.iter()
            .any(|m| m.contains("VerterHost") && m.contains("NOT defined in its named module")),
        "self-test: a QUALIFIED non-authority entry defined only in a WRONG module MUST FIRE; \
         got {v:?}"
    );

    // GREEN: a CATEGORY entry (`ResolvedSurfaceAccess`, a sealed trait bound)
    // carries no def requirement — its absence from the index does NOT fire.
    assert!(
        !present.contains_key("ResolvedSurfaceAccess"),
        "self-test setup: the sealed-trait-bound CATEGORY entry is not a Qualified def"
    );
    assert!(
        non_authority_input_anti_vacuity_violations(&present)
            .iter()
            .all(|m| !m.contains("ResolvedSurfaceAccess")),
        "self-test: a CATEGORY non-authority entry (sealed trait bound) MUST NOT fire anti-vacuity \
         (it carries no def requirement)"
    );

    // The categorization is load-bearing: `ResolvedSurfaceAccess` is the sealed
    // trait bound; `Key` / `Value` / `Discriminant` are generic/assoc/std names;
    // and every CATEGORY entry's name is single-segment-or-PascalCase (a real
    // ident), never an empty placeholder.
    let category_of = |name: &str| -> Option<NonAuthorityCategory> {
        KNOWN_NON_AUTHORITY_INPUT_IDENTS
            .iter()
            .find(|e| e.name() == name)
            .and_then(|e| e.category())
    };
    assert!(
        matches!(
            category_of("ResolvedSurfaceAccess"),
            Some(NonAuthorityCategory::SealedTraitBound(_))
        ),
        "self-test: `ResolvedSurfaceAccess` MUST be categorized as a sealed trait bound"
    );
    for n in ["Key", "Value", "Discriminant"] {
        assert!(
            matches!(
                category_of(n),
                Some(NonAuthorityCategory::GenericOrAssocOrStd(_))
            ),
            "self-test: `{n}` MUST be categorized as a generic/assoc/std name"
        );
    }
    // `VerterHost` is a Qualified entry — it has NO category (justified by its def).
    assert_eq!(
        category_of("VerterHost"),
        None,
        "self-test: a Qualified non-authority entry carries no category"
    );
}

#[test]
fn sink_collector_inline_mod_attribution_self_test_discriminates() {
    // GAP-7: a fn in an inline `mod inner {}` must be recorded under
    // `<file>::inner`, NOT the file's module path — so a `(file, fn)` allowlist
    // entry does NOT match a same-named fn in a different inline submodule.
    let src = r#"
        // A file-scope fn.
        pub(crate) fn at_file_scope(x: u32) -> u32 { x }
        mod inner {
            // A SAME-named fn in an inline submodule — must record under
            // `<base>::inner`, distinct from the file-scope one.
            pub(crate) fn at_file_scope(x: u32) -> u32 { x }
            pub(crate) fn inner_only(x: u32) -> u32 { x }
        }
        mod outer {
            mod nested {
                pub(crate) fn deeply(x: u32) -> u32 { x }
            }
        }
        #[cfg(test)]
        mod tests {
            // A #[cfg(test)] submodule is SKIPPED entirely.
            pub fn test_only(x: u32) -> u32 { x }
        }
    "#;
    let file = syn::parse_file(src).expect("parse inline-mod source");
    let base = "crate::meta_resolve::projectors::output_sink";
    // This test only inspects module attribution, not resolved refs — an empty
    // name index + empty use-index + empty re-export index + empty use-binding
    // index is fine.
    let empty_index: NameDefIndex = BTreeMap::new();
    let empty_reexports = ReExportIndex::default();
    let empty_use_bindings = UseBindingIndex::default();
    let mut collector = SinkFnCollector {
        module_stack: vec![base.to_string()],
        sigs: Vec::new(),
        in_trait_impl: false,
        uses: UseIndex::default(),
        name_to_ids: &empty_index,
        reexports: &empty_reexports,
        use_bindings: &empty_use_bindings,
    };
    syn::visit::Visit::visit_file(&mut collector, &file);

    let recorded: std::collections::BTreeSet<(String, String)> = collector
        .sigs
        .iter()
        .map(|s| (s.module_path.clone(), s.name.clone()))
        .collect();

    // The file-scope fn records under the file base.
    assert!(
        recorded.contains(&(base.to_string(), "at_file_scope".to_string())),
        "self-test: the file-scope fn must record under the file base `{base}`; got {recorded:?}"
    );
    // The inline-submodule fn records under `<base>::inner` — NOT the file base.
    assert!(
        recorded.contains(&(format!("{base}::inner"), "at_file_scope".to_string())),
        "self-test: the inline-submodule fn `inner::at_file_scope` MUST record under \
         `{base}::inner` (GAP-7 module-stack attribution); got {recorded:?}"
    );
    // The two same-named fns are DISTINCT records (the allowlist for one does not
    // cover the other).
    assert!(
        recorded.contains(&(format!("{base}::inner"), "inner_only".to_string())),
        "self-test: `inner::inner_only` must record under `{base}::inner`; got {recorded:?}"
    );
    // A nested inline mod stacks: `<base>::outer::nested`.
    assert!(
        recorded.contains(&(format!("{base}::outer::nested"), "deeply".to_string())),
        "self-test: a nested inline mod must stack the path (`{base}::outer::nested`); \
         got {recorded:?}"
    );
    // The `#[cfg(test)]` submodule's fn is SKIPPED entirely.
    assert!(
        !recorded.iter().any(|(_, n)| n == "test_only"),
        "self-test: a `#[cfg(test)]` inline submodule's fns MUST be skipped; got {recorded:?}"
    );
    // CRITICAL discrimination: the file-base record does NOT collide with the
    // inline-submodule record — an allowlist entry `(file, at_file_scope)` must
    // NOT match the inline-submodule `at_file_scope`.
    let file_scope_paths: Vec<&String> = collector
        .sigs
        .iter()
        .filter(|s| s.name == "at_file_scope")
        .map(|s| &s.module_path)
        .collect();
    assert_eq!(
        file_scope_paths.len(),
        2,
        "self-test: BOTH `at_file_scope` fns must be recorded (file-scope + inline-sub)"
    );
    assert!(
        file_scope_paths.contains(&&base.to_string())
            && file_scope_paths.contains(&&format!("{base}::inner")),
        "self-test: the two `at_file_scope` fns must record under DISTINCT module paths \
         (`{base}` vs `{base}::inner`) so a `(file, fn)` allowlist entry is precise; \
         got {file_scope_paths:?}"
    );
}

#[test]
fn cross_sink_raw_authority_self_test_discriminates() {
    // A synthetic def-graph (qualified ids minted by the resolver from these bare
    // names): the seeds plus a userland DTO that wraps one (`FooSurface { members:
    // Vec<ProjectedMember> }`), proving the closure-driven classification (a
    // NEWLY-NAMED DTO is flagged WITHOUT a hand-list entry). `AdmittedPublishedMember`
    // lands at its canonical publication-authority module (so the closure exclusion
    // keys on its real id).
    let (defs, fixture) = synthetic_defs(&[
        // A userland DTO that wraps a published seed transitively.
        ("FooSurface", &["Vec", "ProjectedMember"]),
        // An alias chain `type AliasedExpr = TypeExpr;` reachable transitively.
        ("AliasedExpr", &["TypeExpr"]),
        // INPUT-side wrapper reaching the forgeable `SurfaceMember` seed.
        ("WrappedSurfaceMember", &["SurfaceMember"]),
        // A genuinely-unrelated wrapper — reaches NO forgeable seed.
        ("UnrelatedWrapper", &["u32", "String"]),
        // An admitted token that WRAPS a forgeable seed internally — EXCLUDED
        // from the forgeable set (the sanctioned admitted input).
        (
            "AdmittedPublishedMember",
            &["SurfaceMember", "ProjectionCursor"],
        ),
        // NEWTYPE of the raw graph handle — a thin rename, IS forgeable authority.
        ("WrappedNodeNewtype", &["SemanticNodeId"]),
        // ALIAS of the raw graph handle — same structural-equivalence rule.
        ("WrappedNodeAlias", &["SemanticNodeId"]),
        // A newtype-of-a-newtype — propagates recursively.
        ("WrappedNodeNewtype2", &["WrappedNodeNewtype"]),
        // MULTI-field infra type holding a node ordinal among other fields —
        // STAYS non-propagating (the 28-FP fence).
        ("CacheWithNode", &["SemanticNodeId", "u64", "bool"]),
        // A multi-field type holding a node ordinal inside a container — still
        // multi-field, still non-propagating.
        ("NodeMap", &["Vec", "SemanticNodeId", "String"]),
    ]);
    let bearing = typeexpr_bearing_closure(&defs);
    let forgeable = forgeable_authority_closure(&defs, &bearing);
    assert!(
        set_contains_name(&bearing, "FooSurface"),
        "self-test: the field-closure MUST flag a newly-named DTO `FooSurface {{ members: \
         Vec<ProjectedMember> }}` as TypeExpr-bearing WITHOUT a spelled-name entry — this is the \
         anti-recurrence property; got {bearing:?}"
    );
    assert!(
        set_contains_name(&bearing, "AliasedExpr"),
        "self-test: the field-closure MUST follow `type AliasedExpr = TypeExpr` aliases; \
         got {bearing:?}"
    );
    // INPUT-closure anti-recurrence: a newly-named wrapper reaching a forgeable
    // seed is flagged WITHOUT a spelled-name entry; an unrelated wrapper is not;
    // an admitted token wrapping a seed is EXCLUDED.
    assert!(
        set_contains_name(&forgeable, "WrappedSurfaceMember"),
        "self-test: the INPUT field-closure MUST flag `WrappedSurfaceMember {{ member: \
         SurfaceMember }}` as forgeable authority WITHOUT a spelled-name entry; got {forgeable:?}"
    );
    assert!(
        !set_contains_name(&forgeable, "UnrelatedWrapper"),
        "self-test: the INPUT field-closure must NOT flag a genuinely-unrelated wrapper; \
         got {forgeable:?}"
    );
    assert!(
        !set_contains_name(&forgeable, "AdmittedPublishedMember"),
        "self-test: an admitted token wrapping a forgeable seed MUST be EXCLUDED from the \
         forgeable-authority set (the sanctioned admitted input is never raw authority); \
         got {forgeable:?}"
    );
    // FIX-8a: a NEWTYPE / ALIAS that is STRUCTURALLY EQUIVALENT to the raw graph
    // handle `SemanticNodeId` (its def's refs, after removing pure container /
    // primitive idents, is the singleton `{SemanticNodeId}`) IS forgeable
    // authority — even though `SemanticNodeId` itself does NOT propagate through
    // multi-field containers. A newtype-of-a-newtype propagates recursively.
    for newtype in [
        "WrappedNodeNewtype",
        "WrappedNodeAlias",
        "WrappedNodeNewtype2",
    ] {
        assert!(
            set_contains_name(&forgeable, newtype),
            "self-test: a newtype/alias structurally EQUIVALENT to the raw `SemanticNodeId` handle \
             (`{newtype}`) MUST be flagged forgeable authority — it is a thin rename of the raw \
             subject (FIX-8a); got {forgeable:?}"
        );
    }
    // FIX-8a (the 28-FP fence): a MULTI-field infra type that merely HOLDS a
    // node ordinal among other fields is NOT a forgeable raw-surface rename and
    // MUST STAY non-propagating.
    for infra in ["CacheWithNode", "NodeMap"] {
        assert!(
            !set_contains_name(&forgeable, infra),
            "self-test: a MULTI-field infra type holding a node ordinal among other fields \
             (`{infra}`) must NOT be flagged forgeable authority (preserves the 28-false-positive \
             fix — `SemanticNodeId` propagates only through a structurally-equivalent \
             newtype/alias, never a multi-field container); got {forgeable:?}"
        );
    }

    // Build sigs against the fixture index, so a fixture-def input
    // (`WrappedSurfaceMember`, `AdmittedPublishedMember`, the node newtypes)
    // resolves to its forgeable / token id while an UNKNOWN name stays unresolved.
    let mk = |module: &str,
              name: &str,
              inputs: &[&str],
              outputs: &[&str],
              mut_out: &[&str],
              test_gated: bool|
     -> SinkFnSig {
        let sig = synthetic_sig(&fixture, module, name, inputs, outputs, mut_out);
        if test_gated {
            sig.marked_test_gated()
        } else {
            sig
        }
    };
    // A MODULE-PRIVATE sink fn (a bare `fn` / `pub(self)` inherent core): same
    // shape as `mk` but provably unreachable cross-sink, so it must be DROPPED.
    let mk_private = |module: &str, name: &str, inputs: &[&str], outputs: &[&str]| -> SinkFnSig {
        synthetic_sig(&fixture, module, name, inputs, outputs, &[]).marked_module_private()
    };

    // KNOWN-GOOD greens: the admitted-token projector path; a sink-local raw
    // helper on the allowlist; a fn returning a DTO but taking only an admitted
    // token; a fn taking forgeable authority but returning a NON-bearing type.
    let greens = vec![
        // Admitted-token publication API — input is a token (not a forgeable
        // seed), output is `ExpandedField`. MUST NOT fire.
        mk(
            "crate::meta_resolve::projectors::output_sink",
            "surface_member_to_expanded_field",
            &["AdmittedPublishedMember"],
            &["ExpandedField"],
            &[],
            false,
        ),
        // Sink-local raiser on the allowlist — forgeable input + TypeExpr out,
        // but explicitly sanctioned.
        mk(
            "crate::project_semantic_dispatch::raise",
            "raise_node_to_type_expr",
            &["SemanticNodeId"],
            &["TypeExpr"],
            &[],
            false,
        ),
        // Candidate reader — forgeable input but output is NOT TypeExpr-bearing
        // (`Vec<SurfaceMemberCandidate>` is an admitted token vector, not a DTO).
        mk(
            "crate::meta_resolve::projectors::publication_authority",
            "read_surface_members",
            &["SemanticNodeId", "ResolvedPayloadSurface"],
            &["SurfaceMemberCandidate"],
            &[],
            false,
        ),
        // A MODULE-PRIVATE node-input core — forgeable `SemanticNodeId` input +
        // `TypeExpr` output, but provably unreachable cross-sink (a bare `fn` /
        // `pub(self)` inherent core). MUST NOT fire: the demand APIs resolve the
        // node internally and the core never crosses the boundary. This is the
        // `materialize_member_surface_node_core` /
        // `projected_expanded_shape_from_node_core` shape.
        mk_private(
            "crate::resolver_core::component_meta_query_engine::registry_decl",
            "materialize_member_surface_node_core",
            &["SemanticNodeId"],
            &["TypeExpr"],
        ),
    ];
    assert!(
        cross_sink_raw_authority_violations(&greens, &bearing, &forgeable).is_empty(),
        "self-test: the known-good greens (admitted-token API + allowlisted raiser + non-bearing \
         candidate reader + module-private node-core) MUST pass; got: {:?}",
        cross_sink_raw_authority_violations(&greens, &bearing, &forgeable)
    );

    // RED set — every §1a planted form. Each MUST fire.
    // (1) raw projector `fn(&SurfaceMember, ProjectionCursor) -> ExpandedField`.
    let red1 = mk(
        "crate::meta_resolve::projectors::output_sink",
        "raw_member_to_field",
        &["SurfaceMember", "ProjectionCursor"],
        &["ExpandedField"],
        &[],
        false,
    );
    // (2) alias-laundered return `type F = ExpandedField; fn(...) -> F` — the
    //     output ident `F` is modelled as a bearing alias: an extended fixture
    //     makes `F` RESOLVE to a synthetic id, and that same id is inserted into
    //     the bearing clone so the output reads bearing.
    let mut fixture_with_alias = fixture.clone();
    fixture_with_alias
        .entry("F".to_string())
        .or_default()
        .insert(synthetic_id("F"));
    let mut bearing_with_alias = bearing.clone();
    bearing_with_alias.insert(synthetic_id("F"));
    let red2 = synthetic_sig(
        &fixture_with_alias,
        "crate::meta_resolve::projectors::props",
        "alias_laundered",
        &["SurfaceMember"],
        &["F"],
        &[],
    );
    // (3) raw `SemanticNodeId -> ExpandedField`.
    let red3 = mk(
        "crate::meta_resolve::projectors::output_sink",
        "raw_node_to_field",
        &["SemanticNodeId"],
        &["ExpandedField"],
        &[],
        false,
    );
    // (4) `&mut ExpandedComponentTypes`-style mutated DTO out-param paired with
    //     a raw subject. Model the DTO as a bearing type via mut_outparam (the
    //     extended fixture makes it resolve; the bearing clone marks it bearing).
    let mut fixture_with_ect = fixture.clone();
    fixture_with_ect
        .entry("ExpandedComponentTypes".to_string())
        .or_default()
        .insert(synthetic_id("ExpandedComponentTypes"));
    let mut bearing_with_ect = bearing.clone();
    bearing_with_ect.insert(synthetic_id("ExpandedComponentTypes"));
    let red4 = synthetic_sig(
        &fixture_with_ect,
        "crate::meta_resolve::projectors::output_sink",
        "mutate_published_surface",
        &["SemanticNodeId", "ExpandedComponentTypes"],
        &[],
        &["ExpandedComponentTypes"],
    );
    // (5) cache ctor taking `&SurfaceMember` and producing a bearing value.
    //     (Modelled as returning `ExpandedField`; the real cache key is
    //     non-bearing, so the guard fires only if a cache ctor returns a DTO.)
    let red5 = mk(
        "crate::component_meta_caches",
        "from_surface_member_raw_leak",
        &["SurfaceMember"],
        &["ExpandedField"],
        &[],
        false,
    );
    // (6) typeinfo `TypeInfoSurfaceMember -> Option<TypeExpr>` (non-allowlisted).
    let red6 = mk(
        "crate::typeinfo::framework_surface::graph_export",
        "leak_member_value",
        &["TypeInfoSurfaceMember"],
        &["Option", "TypeExpr"],
        &[],
        false,
    );
    // (7) `VueMacroSurface -> Vec<AnalyzedPropField>` (non-token normalizer).
    let red7 = mk(
        "crate::typeinfo::framework_surface::graph_export",
        "props_from_forgeable_surface",
        &["VueMacroSurface"],
        &["Vec", "AnalyzedPropField"],
        &[],
        false,
    );
    // (8) `TypeInfoIndexSignature -> ExpandedIndexSignature`.
    let red8 = mk(
        "crate::typeinfo::framework_surface::graph_export",
        "index_sig_leak",
        &["TypeInfoIndexSignature"],
        &["ExpandedIndexSignature"],
        &[],
        false,
    );
    // (9) query-engine `SemanticNodeId -> ProjectedSurface` (non-allowlisted
    //     module — a NEW fn outside the sanctioned `surface` module).
    let red9 = mk(
        "crate::resolver_core::component_meta_query_engine::registry_decl",
        "node_to_projected_surface_leak",
        &["SemanticNodeId"],
        &["ProjectedSurface"],
        &[],
        false,
    );
    // (10) query-engine `&SurfaceView -> ProjectedSurface` (non-allowlisted).
    let red10 = mk(
        "crate::resolver_core::component_meta_query_engine::registry_decl",
        "view_to_projected_surface_leak",
        &["SurfaceView"],
        &["ProjectedSurface"],
        &[],
        false,
    );
    // (11) trait-method form: a trait-method surface pairing forgeable input
    //      with a TypeExpr output (collected via `visit_trait_item_fn`).
    let red11 = mk(
        "crate::meta_resolve::projectors::props",
        "trait_method_leak",
        &["SurfaceMember"],
        &["TypeExpr"],
        &[],
        false,
    );
    // (12) INPUT-side wrapper bypass: a param of a WRAPPER type
    //      (`WrappedSurfaceMember { member: SurfaceMember }`) whose def
    //      transitively reaches a forgeable seed, paired with a TypeExpr output.
    //      The OLD direct-ident-only input check MISSED this; the input
    //      field-closure flags it. `WrappedSurfaceMember` is in `forgeable`.
    let red12 = mk(
        "crate::meta_resolve::projectors::output_sink",
        "wrapped_member_to_field",
        &["WrappedSurfaceMember"],
        &["ExpandedField"],
        &[],
        false,
    );
    // (13) NEWTYPE-of-node bypass (FIX-8a): a param of `WrappedNodeNewtype`
    //      (a `struct WrappedNodeNewtype(SemanticNodeId)` — a thin rename of the
    //      raw subject) paired with a `TypeExpr` output. The OLD code treated
    //      `SemanticNodeId` as non-propagating across ALL containers, so a
    //      newtype of it slipped past; FIX-8a propagates it through a structurally
    //      -equivalent newtype.
    let red13 = mk(
        "crate::meta_resolve::projectors::output_sink",
        "wrapped_node_to_field",
        &["WrappedNodeNewtype"],
        &["TypeExpr"],
        &[],
        false,
    );
    // (14) ALIAS-of-node bypass (FIX-8a): a param of `WrappedNodeAlias`
    //      (`type WrappedNodeAlias = SemanticNodeId`) paired with a `TypeExpr`
    //      output.
    let red14 = mk(
        "crate::meta_resolve::projectors::output_sink",
        "aliased_node_to_field",
        &["WrappedNodeAlias"],
        &["ExpandedField"],
        &[],
        false,
    );
    // (15) PRE-ADMISSION chain-struct bypass (GAP-3): a param of
    //      `SurfaceMemberCandidate` (a sealed construction-chain stage minted
    //      BEFORE `admit_published_member`) paired with an `ExpandedField`
    //      output. It is NOT policy-admitted, so it MUST fire — the GAP-3 split
    //      classifies a pre-admission chain struct as forgeable when taken
    //      directly (distinct from the policy-admitted `AdmittedPublishedMember`,
    //      which is the GREEN `surface_member_to_expanded_field` API above).
    let red15 = mk(
        "crate::meta_resolve::projectors::props",
        "candidate_to_field_bypass",
        &["SurfaceMemberCandidate"],
        &["ExpandedField"],
        &[],
        false,
    );

    for (label, red, b) in [
        (
            "raw &SurfaceMember+cursor -> ExpandedField",
            &red1,
            &bearing,
        ),
        (
            "alias-laundered -> F(=ExpandedField)",
            &red2,
            &bearing_with_alias,
        ),
        ("raw SemanticNodeId -> ExpandedField", &red3, &bearing),
        (
            "&mut ExpandedComponentTypes + raw subject",
            &red4,
            &bearing_with_ect,
        ),
        ("cache ctor &SurfaceMember -> DTO", &red5, &bearing),
        (
            "typeinfo TypeInfoSurfaceMember -> Option<TypeExpr>",
            &red6,
            &bearing,
        ),
        ("VueMacroSurface -> Vec<AnalyzedPropField>", &red7, &bearing),
        (
            "TypeInfoIndexSignature -> ExpandedIndexSignature",
            &red8,
            &bearing,
        ),
        ("query SemanticNodeId -> ProjectedSurface", &red9, &bearing),
        ("query &SurfaceView -> ProjectedSurface", &red10, &bearing),
        ("trait-method SurfaceMember -> TypeExpr", &red11, &bearing),
        (
            "WRAPPER WrappedSurfaceMember -> ExpandedField (input-closure bypass)",
            &red12,
            &bearing,
        ),
        (
            "NEWTYPE WrappedNodeNewtype(SemanticNodeId) -> TypeExpr (FIX-8a)",
            &red13,
            &bearing,
        ),
        (
            "ALIAS WrappedNodeAlias = SemanticNodeId -> ExpandedField (FIX-8a)",
            &red14,
            &bearing,
        ),
        (
            "PRE-ADMISSION chain-struct SurfaceMemberCandidate -> ExpandedField (GAP-3)",
            &red15,
            &bearing,
        ),
    ] {
        let mut planted = greens.clone();
        planted.push(red.clone());
        let v = cross_sink_raw_authority_violations(&planted, b, &forgeable);
        assert!(
            v.iter().any(|m| m.contains(&red.name)),
            "self-test: planted RED `{label}` (`{}`) MUST FIRE the structural boundary check; \
             got: {v:?}",
            red.name
        );
    }

    // FIX-8a fence GREEN: a fn taking the MULTI-field infra type `CacheWithNode`
    // (a node ordinal among other fields, NOT a forgeable rename) and returning
    // `TypeExpr` must NOT fire — `CacheWithNode` is non-forgeable, so this is an
    // ordinary infra accessor, not a raw-authority → TypeExpr boundary.
    let infra_green = mk(
        "crate::meta_resolve::projectors::output_sink",
        "cache_node_lookup",
        &["CacheWithNode"],
        &["TypeExpr"],
        &[],
        false,
    );
    assert!(
        cross_sink_raw_authority_violations(&[infra_green], &bearing, &forgeable).is_empty(),
        "self-test: a fn taking the multi-field infra type `CacheWithNode` (non-forgeable) and \
         returning `TypeExpr` must NOT fire (the 28-FP fence — only a structurally-equivalent \
         newtype/alias of `SemanticNodeId` propagates)"
    );

    // GAP-3 GREEN: a NON-allowlisted fn taking ONLY the policy-admitted token
    // `AdmittedPublishedMember` and returning `ExpandedField` must NOT fire — the
    // policy-admitted token is a genuinely-safe input (NOT via the allowlist; via
    // the closure exclusion). This is the exact counterpart to the GAP-3 RED
    // `red15` (a PRE-admission chain struct in the same shape DOES fire), proving
    // the split distinguishes policy-admitted from pre-admission.
    let admitted_green = mk(
        "crate::meta_resolve::projectors::props",
        "admitted_token_to_field_ok",
        &["AdmittedPublishedMember"],
        &["ExpandedField"],
        &[],
        false,
    );
    assert!(
        cross_sink_raw_authority_violations(&[admitted_green], &bearing, &forgeable).is_empty(),
        "self-test: a NON-allowlisted fn taking ONLY the policy-admitted `AdmittedPublishedMember` \
         token and returning `ExpandedField` must NOT fire (it is a genuinely-safe policy-admitted \
         input — the GAP-3 GREEN counterpart to the pre-admission chain-struct RED)"
    );

    // ANTI-VACUITY (token green stays green even when a sibling RED is present):
    // the admitted-token API is NOT reported among the violations.
    let mut mixed = greens.clone();
    mixed.push(red1.clone());
    let v = cross_sink_raw_authority_violations(&mixed, &bearing, &forgeable);
    assert!(
        !v.iter()
            .any(|m| m.contains("surface_member_to_expanded_field")),
        "self-test: the admitted-token GREEN API must NOT be reported as a violation even \
         alongside a planted RED; got: {v:?}"
    );

    // A `#[cfg(test)]`-gated raw fn is DROPPED (production-only scan).
    let test_gated_red = mk(
        "crate::meta_resolve::projectors::output_sink",
        "raw_node_to_field_test_only",
        &["SemanticNodeId"],
        &["ExpandedField"],
        &[],
        true,
    );
    assert!(
        cross_sink_raw_authority_violations(&[test_gated_red], &bearing, &forgeable).is_empty(),
        "self-test: a #[cfg(test)]-gated raw fn MUST be dropped (production-only scan)"
    );

    // A MODULE-PRIVATE raw fn is DROPPED even with a forgeable input + bearing
    // output — it is unreachable cross-sink (the demand-API closure of FIX-A).
    let private_red = mk_private(
        "crate::resolver_core::component_meta_query_engine::registry_decl",
        "projected_expanded_shape_from_node_core",
        &["SemanticNodeId"],
        &["ExpandedObjectShape"],
    );
    assert!(
        cross_sink_raw_authority_violations(&[private_red], &bearing, &forgeable).is_empty(),
        "self-test: a module-private node-input core MUST be dropped (unreachable cross-sink; the \
         demand APIs resolve the node internally)"
    );
}

#[test]
fn qualified_typepath_node_newtype_self_test_discriminates() {
    // A node newtype / alias written with a FULLY-QUALIFIED path
    // (`crate::semantic_query::SemanticNodeId`) must classify as a raw-node
    // singleton — the module-qualified resolved identity, not a bare ident set.
    // The bare-ident-set reading would harvest `{crate, semantic_query,
    // SemanticNodeId}` (three idents) and FAIL the singleton test. Drive the REAL
    // collector (`type_segment_refs` + `TypeDefCollector` + the resolver) on actual
    // source so the test exercises the production identity logic, not a shortcut.
    let src = r#"
        struct W(crate::semantic_query::SemanticNodeId);
        type W2 = crate::semantic_query::SemanticNodeId;
        // The exact production shape this gap was found against.
        struct MemberShapeNodeSubject(crate::semantic_query::SemanticNodeId);
        // A path-qualified Vec wrapper of the handle (still a thin rename).
        struct W3(Vec<semantic_query::SemanticNodeId>);
        // A MULTI-field infra type holding a qualified node among other fields —
        // must STAY non-forgeable (the 28-FP fence survives qualified paths).
        struct Cache { node: crate::semantic_query::SemanticNodeId, gen: u64 }
    "#;
    let file = syn::parse_file(src).expect("parse qualified-path source");
    let mut collector = TypeDefCollector::with_module_base("crate::component_meta_caches".into());
    syn::visit::Visit::visit_file(&mut collector, &file);
    let (defs, _) = defs_from_collector(&collector);

    // The qualified resolution: `W`'s sole field ref RESOLVES to the
    // `crate::semantic_query::SemanticNodeId` id (NOT three bare path idents), so
    // it is a singleton rename of the raw handle.
    let w_id = TypeDefId::new("crate::component_meta_caches", "W");
    let w_refs = &defs[&w_id].refs;
    let node_id = TypeDefId::new("crate::semantic_query", "SemanticNodeId");
    assert!(
        w_refs.len() == 1 && w_refs.iter().next().unwrap().resolved() == Some(&node_id),
        "self-test: a `crate::semantic_query::SemanticNodeId` field must resolve to the qualified \
         `SemanticNodeId` id (a single subject, the module-qualifier segments dropped); got {w_refs:?}"
    );

    let bearing = typeexpr_bearing_closure(&defs);
    let forgeable = forgeable_authority_closure(&defs, &bearing);
    for newtype in ["W", "W2", "MemberShapeNodeSubject", "W3"] {
        assert!(
            set_contains_name(&forgeable, newtype),
            "self-test: the qualified-path node newtype/alias `{newtype}` MUST be flagged forgeable \
             authority (qualified singleton identity — GAP-1); got {forgeable:?}"
        );
    }
    assert!(
        !set_contains_name(&forgeable, "Cache"),
        "self-test: a MULTI-field infra type holding a QUALIFIED node among other fields \
         (`Cache`) must STAY non-forgeable (the 28-FP fence survives qualified paths); \
         got {forgeable:?}"
    );

    // The boundary check fires on a sink fn taking the qualified-path newtype and
    // returning a `TypeExpr`-bearing value (the MemberShapeNodeSubject shape). The
    // input resolves through the fixture (`MemberShapeNodeSubject` is forgeable);
    // the output `ExpandedField` resolves via the seed merge to bearing.
    let (_, name_to_ids) = defs_from_collector(&collector);
    let red = synthetic_sig(
        &name_to_ids,
        "crate::component_meta_caches",
        "node_subject_to_field",
        &["MemberShapeNodeSubject"],
        &["ExpandedField"],
        &[],
    );
    let v = cross_sink_raw_authority_violations(&[red], &bearing, &forgeable);
    assert!(
        v.iter().any(|m| m.contains("node_subject_to_field")),
        "self-test: a sink fn taking the qualified-path node newtype \
         `MemberShapeNodeSubject(crate::semantic_query::SemanticNodeId)` and returning a bearing \
         value MUST FIRE (GAP-1 — the bare-ident-set reading missed it); got {v:?}"
    );
}

#[test]
fn qualified_safe_input_identity_self_test_discriminates() {
    // The safe-input / construction-chain exemptions are keyed by MODULE-QUALIFIED
    // identity, and the collision check is now PURE ANTI-VACUITY: a sanctioned
    // token must be defined at its exact `(module, name)`. Under qualified
    // identity a bare name shared by two modules is simply two DISTINCT ids — the
    // `ResolvedMacroPayload` sealed `publication_authority` token and the bearing
    // `results` DTO alias coexist as distinct ids, NOT one masked bare slot.

    // Build a name index with every sanctioned token at its canonical home, PLUS
    // the `results` `ResolvedMacroPayload` collision (an unrelated DTO alias).
    let mut name_to_ids: NameDefIndex = BTreeMap::new();
    for (module, name) in SEALED_CONSTRUCTION_CHAIN_STRUCTS
        .iter()
        .chain(POLICY_ADMITTED_SAFE_INPUTS.iter())
    {
        name_to_ids
            .entry((*name).to_string())
            .or_default()
            .insert(TypeDefId::new(*module, *name));
    }
    let results_rmp = TypeDefId::new(
        "crate::typeinfo::framework_surface::results",
        "ResolvedMacroPayload",
    );
    let chain_rmp = TypeDefId::new(
        "crate::meta_resolve::projectors::publication_authority",
        "ResolvedMacroPayload",
    );
    name_to_ids
        .get_mut("ResolvedMacroPayload")
        .unwrap()
        .insert(results_rmp.clone());

    // DISTINCT-IDS CRUX: the two `ResolvedMacroPayload` defs are DISTINCT ids in
    // the index (the chain token vs the bearing alias), NOT one merged bare slot.
    let rmp_ids = &name_to_ids["ResolvedMacroPayload"];
    assert!(
        rmp_ids.contains(&chain_rmp) && rmp_ids.contains(&results_rmp) && rmp_ids.len() == 2,
        "self-test: `ResolvedMacroPayload` must be TWO distinct module-qualified ids (the sealed \
         publication-authority token + the bearing `results` alias), not one bare-name slot; \
         got {rmp_ids:?}"
    );

    // (a) GREEN: with every sanctioned token present at its exact home (and the
    // collision present), the anti-vacuity check passes — the collision is no
    // longer "accepted because bearing"; the distinct ids each stand on their own.
    assert!(
        qualified_safe_input_collision_violations(&name_to_ids).is_empty(),
        "self-test: with every sanctioned token at its exact `(module, name)` (and a distinct \
         colliding id present), the anti-vacuity check MUST pass; got {:?}",
        qualified_safe_input_collision_violations(&name_to_ids)
    );

    // (b) RED anti-vacuity: a sanctioned token with NO definition at all fires.
    let mut missing = name_to_ids.clone();
    missing.remove("AdmittedPublishedMember");
    let v = qualified_safe_input_collision_violations(&missing);
    assert!(
        v.iter()
            .any(|m| m.contains("AdmittedPublishedMember") && m.contains("NO definition")),
        "self-test: a sanctioned safe-input token with NO definition in the read roots MUST FIRE \
         (anti-vacuity — a rename/move surfaces loudly); got {v:?}"
    );

    // (c) RED: a sanctioned token defined ONLY in a WRONG module fires (the exact
    // `(module, name)` no longer matches a collected id).
    let mut moved = name_to_ids.clone();
    moved.insert(
        "AdmittedPublishedMember".to_string(),
        [TypeDefId::new(
            "crate::some::other::place",
            "AdmittedPublishedMember",
        )]
        .into_iter()
        .collect(),
    );
    let v = qualified_safe_input_collision_violations(&moved);
    assert!(
        v.iter()
            .any(|m| m.contains("AdmittedPublishedMember") && m.contains("NOT defined in its")),
        "self-test: a sanctioned safe-input token defined only in a non-sanctioned module MUST FIRE \
         (the token moved); got {v:?}"
    );
}

#[test]
fn qualified_index_signature_identity_self_test_discriminates() {
    // The genuine-qualified-identity discriminating set. The CRUX: the two
    // `IndexSignature` defs — `crate::semantic_query::IndexSignature` (its
    // `key_type`/`value_type` fields are `SemanticNodeId`, so it is an
    // INPUT-authority seed) and `verter_type_expr::IndexSignature` (its
    // `key_type`/`value_type` are `TypeExpr`, so it is an already-lowered-IR
    // OUTPUT-side bearing leaf) — must classify on their OWN merits, never
    // merged into one bare-name node.
    let sem_index = TypeDefId::new("crate::semantic_query", "IndexSignature");
    let te_index = TypeDefId::new("verter_type_expr", "IndexSignature");

    // Build a def-graph DIRECTLY (explicit qualified ids): the two IndexSignatures
    // plus a wrapper around EACH.
    let m = "crate::test_synthetic";
    let mk = |refs: &[TypeRef]| TypeDefRefs {
        refs: refs.iter().cloned().collect(),
    };
    let defs: BTreeMap<TypeDefId, TypeDefRefs> = [
        // The semantic-query IndexSignature: holds SemanticNodeId handles.
        (
            sem_index.clone(),
            mk(&[resolved_ref("crate::semantic_query", "SemanticNodeId")]),
        ),
        // The type-expr IndexSignature: holds TypeExpr directly (bearing).
        (
            te_index.clone(),
            mk(&[resolved_ref("verter_type_expr", "TypeExpr")]),
        ),
        // (2) A wrapper that carries the AUTHORITY semantic IndexSignature.
        (
            TypeDefId::new(m, "AuthorityWrapper"),
            mk(&[resolved_ref("crate::semantic_query", "IndexSignature")]),
        ),
        // (3) A wrapper that carries the type-expr (bearing) IndexSignature.
        (
            TypeDefId::new(m, "IrSignatureHolder"),
            mk(&[resolved_ref("verter_type_expr", "IndexSignature")]),
        ),
    ]
    .into_iter()
    .collect();
    let bearing = typeexpr_bearing_closure(&defs);
    let forgeable = forgeable_authority_closure(&defs, &bearing);

    // (1) DISTINCT IDS: the closure graph has TWO IndexSignature entries, not one.
    let index_keys: Vec<&TypeDefId> = defs
        .keys()
        .filter(|id| id.name == "IndexSignature")
        .collect();
    assert_eq!(
        index_keys.len(),
        2,
        "self-test (1): the two `IndexSignature` defs MUST be DISTINCT module-qualified ids in the \
         closure graph (not one merged bare-name node); got {index_keys:?}"
    );
    assert!(
        forgeable.contains(&sem_index) && bearing.contains(&te_index),
        "self-test (1): the semantic-query IndexSignature is forgeable raw-authority; the type-expr \
         IndexSignature is bearing — DISTINCT classifications"
    );
    assert!(
        !bearing.contains(&sem_index) && !forgeable.contains(&te_index),
        "self-test (1): the semantic-query IndexSignature is NOT bearing and the type-expr \
         IndexSignature is NOT forgeable raw-authority — a bare-name merge would conflate them"
    );

    // (2) A wrapper around the AUTHORITY IndexSignature is forgeable; used as a
    // sink input returning a bearing output, the cross-sink check FIRES.
    assert!(
        forgeable.contains(&TypeDefId::new(m, "AuthorityWrapper")),
        "self-test (2): a wrapper around the authority `crate::semantic_query::IndexSignature` MUST \
         be forgeable raw-authority; got {forgeable:?}"
    );
    let mut fixture: NameDefIndex = BTreeMap::new();
    for name in ["AuthorityWrapper", "IrSignatureHolder"] {
        fixture
            .entry(name.to_string())
            .or_default()
            .insert(TypeDefId::new(m, name));
    }
    let red_wrapper = synthetic_sig(
        &fixture,
        "crate::meta_resolve::projectors::output_sink",
        "authority_wrapper_to_field",
        &["AuthorityWrapper"],
        &["ExpandedField"],
        &[],
    );
    let v = cross_sink_raw_authority_violations(&[red_wrapper], &bearing, &forgeable);
    assert!(
        v.iter().any(|msg| msg.contains("authority_wrapper_to_field")),
        "self-test (2): a sink fn taking a wrapper of the authority IndexSignature and returning a \
         bearing value MUST FIRE; got {v:?}"
    );

    // (3) A wrapper around the type-expr (bearing) IndexSignature is BEARING but
    // NOT forgeable raw-authority — an already-lowered-IR holder. A `fn(&W2)->()`
    // does NOT fire as a forgeable input.
    assert!(
        bearing.contains(&TypeDefId::new(m, "IrSignatureHolder"))
            && !forgeable.contains(&TypeDefId::new(m, "IrSignatureHolder")),
        "self-test (3): a wrapper around the type-expr IndexSignature is bearing (output-side) but \
         NOT forgeable raw-authority (it is an already-lowered-IR holder)"
    );
    let green_ir = synthetic_sig(
        &fixture,
        "crate::meta_resolve::projectors::output_sink",
        "ir_holder_accessor",
        &["IrSignatureHolder"],
        &["bool"],
        &[],
    );
    let v = cross_sink_raw_authority_violations(&[green_ir], &bearing, &forgeable);
    assert!(
        v.is_empty(),
        "self-test (3): a fn taking the type-expr-IndexSignature holder and returning a non-bearing \
         value must NOT fire as a forgeable input; got {v:?}"
    );

    // (4) An AMBIGUOUS UNQUALIFIED `IndexSignature` reference on a sink boundary
    // — with both homes in the name index and no local/import disambiguation —
    // resolves to `Unresolved` (the resolver cannot prove a single target).
    let mut both: NameDefIndex = BTreeMap::new();
    both.entry("IndexSignature".to_string())
        .or_default()
        .extend([sem_index.clone(), te_index.clone()]);
    let ambiguous = resolve_type_ref(
        &["IndexSignature".to_string()],
        "crate::meta_resolve::projectors::output_sink",
        &UseIndex::default(),
        &both,
        &ReExportIndex::default(),
        &UseBindingIndex::default(),
    );
    assert!(
        ambiguous.resolved().is_none(),
        "self-test (4): an UNQUALIFIED `IndexSignature` ref with two colliding homes and no \
         disambiguation MUST stay Unresolved (fail-closed at the boundary); got {ambiguous:?}"
    );
    // ...and a QUALIFIED reference to EACH resolves to the right DISTINCT id.
    let sem_ref = resolve_type_ref(
        &[
            "crate".to_string(),
            "semantic_query".to_string(),
            "IndexSignature".to_string(),
        ],
        "crate::meta_resolve::projectors::output_sink",
        &UseIndex::default(),
        &both,
        &ReExportIndex::default(),
        &UseBindingIndex::default(),
    );
    let te_ref = resolve_type_ref(
        &["verter_type_expr".to_string(), "IndexSignature".to_string()],
        "crate::meta_resolve::projectors::output_sink",
        &UseIndex::default(),
        &both,
        &ReExportIndex::default(),
        &UseBindingIndex::default(),
    );
    assert_eq!(
        sem_ref.resolved(),
        Some(&sem_index),
        "self-test (4): `crate::semantic_query::IndexSignature` resolves to the semantic-query id"
    );
    assert_eq!(
        te_ref.resolved(),
        Some(&te_index),
        "self-test (4): `verter_type_expr::IndexSignature` resolves to the type-expr id"
    );
}

#[test]
fn qualified_resolved_macro_payload_identity_self_test_discriminates() {
    // (5) The projector-token `ResolvedMacroPayload` (publication_authority) vs
    // the `results` DTO ALIAS of the same bare name resolve to DISTINCT ids; the
    // safe-input exemption matches ONLY the publication_authority `(module,name)`,
    // and the collision is no longer "accepted because one is bearing".
    let token = TypeDefId::new(
        "crate::meta_resolve::projectors::publication_authority",
        "ResolvedMacroPayload",
    );
    let alias = TypeDefId::new(
        "crate::typeinfo::framework_surface::results",
        "ResolvedMacroPayload",
    );

    // A name index with BOTH homes (the construction-chain token + the bearing
    // results alias).
    let mut name_to_ids: NameDefIndex = BTreeMap::new();
    name_to_ids
        .entry("ResolvedMacroPayload".to_string())
        .or_default()
        .extend([token.clone(), alias.clone()]);

    // The safe-input / chain exemption is keyed by qualified identity: the
    // sanctioned chain `(module,name)` is the publication_authority token ONLY.
    let chain = sealed_construction_chain_ids();
    assert!(
        chain.contains(&token) && !chain.contains(&alias),
        "self-test (5): the construction-chain exemption matches ONLY the publication_authority \
         `ResolvedMacroPayload` token, NOT the bearing `results` alias"
    );

    // A QUALIFIED reference to each resolves to its DISTINCT id.
    let token_ref = resolve_type_ref(
        &[
            "crate".to_string(),
            "meta_resolve".to_string(),
            "projectors".to_string(),
            "publication_authority".to_string(),
            "ResolvedMacroPayload".to_string(),
        ],
        "crate::meta_resolve::projectors::props",
        &UseIndex::default(),
        &name_to_ids,
        &ReExportIndex::default(),
        &UseBindingIndex::default(),
    );
    let alias_ref = resolve_type_ref(
        &[
            "crate".to_string(),
            "typeinfo".to_string(),
            "framework_surface".to_string(),
            "results".to_string(),
            "ResolvedMacroPayload".to_string(),
        ],
        "crate::meta_resolve::projectors::props",
        &UseIndex::default(),
        &name_to_ids,
        &ReExportIndex::default(),
        &UseBindingIndex::default(),
    );
    assert_eq!(
        token_ref.resolved(),
        Some(&token),
        "self-test (5): the publication_authority `ResolvedMacroPayload` resolves to the token id"
    );
    assert_eq!(
        alias_ref.resolved(),
        Some(&alias),
        "self-test (5): the `results` `ResolvedMacroPayload` resolves to the alias id (DISTINCT)"
    );

    // A sink fn taking the construction-chain token DIRECTLY (qualified) and
    // returning a bearing output FIRES (it bypassed the policy gate); a fn taking
    // the bearing results alias does NOT fire as a forgeable input (it is bearing
    // output, classified on its own merits — never a forgeable raw-authority
    // input).
    let mut fixture = name_to_ids.clone();
    fixture
        .entry("ExpandedField".to_string())
        .or_default()
        .insert(synthetic_id("ExpandedField"));
    // Resolve the token / alias param qualified so the right id is the input.
    let bearing: std::collections::BTreeSet<TypeDefId> =
        [synthetic_id("ExpandedField")].into_iter().collect();
    let forgeable: std::collections::BTreeSet<TypeDefId> = std::collections::BTreeSet::new();
    let chain_input_sig = SinkFnSig {
        module_path: "crate::meta_resolve::projectors::props".to_string(),
        name: "chain_token_to_field".to_string(),
        input_idents: [TypeRef::Resolved(token.clone())].into_iter().collect(),
        output_idents: [TypeRef::Resolved(synthetic_id("ExpandedField"))]
            .into_iter()
            .collect(),
        mut_outparam_idents: std::collections::BTreeSet::new(),
        test_gated: false,
        module_private: false,
    };
    let v = cross_sink_raw_authority_violations(&[chain_input_sig], &bearing, &forgeable);
    assert!(
        v.iter().any(|m| m.contains("chain_token_to_field")),
        "self-test (5): a sink fn taking the publication_authority `ResolvedMacroPayload` chain \
         token DIRECTLY and returning a bearing value MUST FIRE (it bypassed the policy gate); \
         got {v:?}"
    );
    let alias_input_sig = SinkFnSig {
        module_path: "crate::meta_resolve::projectors::props".to_string(),
        name: "alias_to_field".to_string(),
        input_idents: [TypeRef::Resolved(alias.clone())].into_iter().collect(),
        output_idents: [TypeRef::Resolved(synthetic_id("ExpandedField"))]
            .into_iter()
            .collect(),
        mut_outparam_idents: std::collections::BTreeSet::new(),
        test_gated: false,
        module_private: false,
    };
    let v = cross_sink_raw_authority_violations(&[alias_input_sig], &bearing, &forgeable);
    assert!(
        v.is_empty(),
        "self-test (5): a sink fn taking the bearing `results` `ResolvedMacroPayload` alias is NOT \
         a forgeable raw-authority input (it is a bearing output type, classified on its own \
         merits) — must NOT fire; got {v:?}"
    );
}

#[test]
fn qualified_use_rename_alias_resolves_self_test_discriminates() {
    // The use-rename gap: `use crate::semantic_query::SemanticNodeId as NodeId;`
    // then `struct W(NodeId)` must resolve `NodeId` →
    // `crate::semantic_query::SemanticNodeId`, so the wrapper is a forgeable
    // raw-node singleton (and a sink fn taking it FIRES).
    let src = r#"
        use crate::semantic_query::SemanticNodeId as NodeId;
        struct RenamedNodeWrapper(NodeId);
    "#;
    let file = syn::parse_file(src).expect("parse use-rename source");
    let module = "crate::component_meta_caches";
    let mut collector = TypeDefCollector::with_module_base(module.to_string());
    syn::visit::Visit::visit_file(&mut collector, &file);
    let uses = collect_use_index(&file);

    // The use index maps `NodeId` -> the qualified SemanticNodeId path.
    assert_eq!(
        uses.unique_path("NodeId"),
        Some(&vec![
            "crate".to_string(),
            "semantic_query".to_string(),
            "SemanticNodeId".to_string(),
        ]),
        "self-test: the use-rename `SemanticNodeId as NodeId` must index `NodeId` to the qualified \
         path"
    );

    // Resolve `RenamedNodeWrapper`'s field ref `NodeId` against the file imports:
    // it RESOLVES to the SemanticNodeId id (the rename alias is followed).
    let mut name_to_ids: NameDefIndex = BTreeMap::new();
    for id in collector.raw_defs.keys() {
        name_to_ids
            .entry(id.name.clone())
            .or_default()
            .insert(id.clone());
    }
    let resolve_index = name_index_with_seed_ids(&name_to_ids);
    let node_id = TypeDefId::new("crate::semantic_query", "SemanticNodeId");
    let resolved = resolve_type_ref(
        &["NodeId".to_string()],
        module,
        &uses,
        &resolve_index,
        &ReExportIndex::default(),
        &UseBindingIndex::default(),
    );
    assert_eq!(
        resolved.resolved(),
        Some(&node_id),
        "self-test: an unqualified `NodeId` (a use-rename of SemanticNodeId) MUST resolve through \
         the file import to `crate::semantic_query::SemanticNodeId`; got {resolved:?}"
    );

    // The wrapper is therefore a forgeable raw-node singleton, and a sink fn
    // taking it + returning a bearing value FIRES.
    let mut defs: BTreeMap<TypeDefId, TypeDefRefs> = BTreeMap::new();
    defs.insert(
        TypeDefId::new(module, "RenamedNodeWrapper"),
        TypeDefRefs {
            refs: [resolved].into_iter().collect(),
        },
    );
    let bearing = typeexpr_bearing_closure(&defs);
    let forgeable = forgeable_authority_closure(&defs, &bearing);
    assert!(
        forgeable.contains(&TypeDefId::new(module, "RenamedNodeWrapper")),
        "self-test: a `struct RenamedNodeWrapper(NodeId)` where `NodeId` is a use-rename of \
         `SemanticNodeId` MUST be a forgeable raw-node singleton; got {forgeable:?}"
    );
}

#[test]
fn qualified_path_requires_matching_qualifier_self_test_discriminates() {
    // The CONSERVATIVE FAIL-CLOSED qualified-path property: a QUALIFIED reference
    // (`>=2` segments) resolves a UNIQUE-name candidate ONLY when the written
    // qualifier is PROVED to name it (a SUFFIX-or-equal module match, OR a proven
    // `pub`/`pub(crate)` re-export — an ANCESTOR prefix is NOT proof) — NEVER on
    // uniqueness alone. A fabricated
    // `external::AdmittedPublishedMember` qualifier over the unique sanctioned
    // token MUST stay `Unresolved`, so a sink fn taking it is caught fail-closed
    // (forgeable-input detection / unclassifiable-input completeness) rather than
    // blessed as a safe admitted token. (The prior fail-OPEN unique-name shortcut
    // resolved it → both detections were skipped; this discriminates that.)
    let token = TypeDefId::new(
        "crate::meta_resolve::projectors::publication_authority",
        "AdmittedPublishedMember",
    );
    // Index: the sanctioned token is the UNIQUE `AdmittedPublishedMember` home,
    // plus the bearing seed `ExpandedField` (via the seed merge).
    let mut name_to_ids: NameDefIndex = BTreeMap::new();
    name_to_ids
        .entry("AdmittedPublishedMember".to_string())
        .or_default()
        .insert(token.clone());
    let resolve_index = name_index_with_seed_ids(&name_to_ids);

    // RED (resolver level): a fabricated `external::AdmittedPublishedMember`
    // qualifier over the UNIQUE token MUST stay `Unresolved` — uniqueness alone
    // does not resolve a qualified path. (Pre-fix: the unique-name shortcut
    // resolved it → this assertion FAILS.)
    let forged = resolve_type_ref(
        &[
            "external".to_string(),
            "AdmittedPublishedMember".to_string(),
        ],
        "crate::meta_resolve::projectors::props",
        &UseIndex::default(),
        &resolve_index,
        &ReExportIndex::default(),
        &UseBindingIndex::default(),
    );
    assert!(
        forged.resolved().is_none(),
        "self-test: a fabricated `external::AdmittedPublishedMember` qualifier over the UNIQUE \
         sanctioned token MUST stay Unresolved (a qualified path resolves a unique candidate only \
         when the qualifier is proved to name it) — uniqueness alone is the fail-OPEN bug; \
         got {forged:?}"
    );

    // GREEN (resolver level, positive): a GENUINE written-qualified
    // `crate::meta_resolve::projectors::publication_authority::AdmittedPublishedMember`
    // ref still resolves to the token (the FULL qualifier equals — a suffix-or-
    // equal match of — its real module).
    let genuine = resolve_type_ref(
        &[
            "crate".to_string(),
            "meta_resolve".to_string(),
            "projectors".to_string(),
            "publication_authority".to_string(),
            "AdmittedPublishedMember".to_string(),
        ],
        "crate::meta_resolve::projectors::props",
        &UseIndex::default(),
        &resolve_index,
        &ReExportIndex::default(),
        &UseBindingIndex::default(),
    );
    assert_eq!(
        genuine.resolved(),
        Some(&token),
        "self-test: a genuine written-qualified `…::publication_authority::AdmittedPublishedMember` \
         ref MUST still resolve to the token (the qualifier matches its real module); got {genuine:?}"
    );

    // GREEN (resolver level, re-export positive): a written re-export path whose
    // qualifier denotes a re-exporting module resolves to the def via the proven
    // re-export rail (a UNIQUE name through a matching re-export).
    let mut reexports = ReExportIndex::default();
    reexports
        .entry(TypeDefId::new(
            "crate::meta_resolve::projectors",
            "AdmittedPublishedMember",
        ))
        .or_default()
        .insert(vec![
            "self".to_string(),
            "publication_authority".to_string(),
            "AdmittedPublishedMember".to_string(),
        ]);
    let via_reexport = resolve_type_ref(
        &[
            "crate".to_string(),
            "meta_resolve".to_string(),
            "projectors".to_string(),
            "AdmittedPublishedMember".to_string(),
        ],
        "crate::meta_resolve::projectors::props",
        &UseIndex::default(),
        &resolve_index,
        &reexports,
        &UseBindingIndex::default(),
    );
    assert_eq!(
        via_reexport.resolved(),
        Some(&token),
        "self-test: a written re-export path `crate::meta_resolve::projectors::\
         AdmittedPublishedMember` (re-exported from `publication_authority`) MUST resolve to the \
         token via the proven re-export rail; got {via_reexport:?}"
    );

    // RED (the re-export rail is genuinely LOAD-BEARING): the SAME
    // ancestor-shortened path resolved through an EMPTY `ReExportIndex::default()`
    // MUST stay `Unresolved`. The written qualifier
    // `["crate","meta_resolve","projectors"]` is an ANCESTOR PREFIX of the token's
    // real module `crate::meta_resolve::projectors::publication_authority` (a
    // prefix, NOT a suffix). With the deleted `|| prefix` arm of
    // `module_qualifier_matches` it resolved to the token WITHOUT any re-export
    // (so this assertion FAILS — the prefix arm short-circuits the rail and the
    // rail is not load-bearing). With `module_qualifier_matches` now
    // suffix-or-equal ONLY, an ancestor-shortened qualifier resolves ONLY via the
    // proven re-export — an empty index leaves it `Unresolved`.
    let via_empty_reexport = resolve_type_ref(
        &[
            "crate".to_string(),
            "meta_resolve".to_string(),
            "projectors".to_string(),
            "AdmittedPublishedMember".to_string(),
        ],
        "crate::meta_resolve::projectors::props",
        &UseIndex::default(),
        &resolve_index,
        &ReExportIndex::default(),
        &UseBindingIndex::default(),
    );
    assert!(
        via_empty_reexport.resolved().is_none(),
        "self-test (rail load-bearing): the ancestor-shortened qualifier \
         `crate::meta_resolve::projectors::AdmittedPublishedMember` MUST stay Unresolved with an \
         EMPTY re-export index (its qualifier is an ANCESTOR PREFIX of the real module, never a \
         suffix). If it resolves here, the deleted `|| prefix` arm is short-circuiting the \
         re-export rail and the rail is not load-bearing; got {via_empty_reexport:?}"
    );

    // RED (ancestor-prefix is NOT proof): an even-shorter ancestor qualifier
    // `crate::AdmittedPublishedMember` over the unique token MUST stay
    // `Unresolved` (no proven re-export). With the deleted `|| prefix` arm
    // `["crate"]` prefix-matched the token's deep real module and resolved it
    // WITHOUT any re-export (this assertion FAILS pre-fix). Post-fix `["crate"]`
    // is neither a suffix of nor equal to the real module, and no re-export
    // proves it, so it stays `Unresolved`.
    let via_crate_ancestor = resolve_type_ref(
        &["crate".to_string(), "AdmittedPublishedMember".to_string()],
        "crate::meta_resolve::projectors::props",
        &UseIndex::default(),
        &resolve_index,
        &ReExportIndex::default(),
        &UseBindingIndex::default(),
    );
    assert!(
        via_crate_ancestor.resolved().is_none(),
        "self-test (ancestor-prefix is not proof): `crate::AdmittedPublishedMember` (a too-short \
         ANCESTOR qualifier) MUST stay Unresolved over the unique token — `[\"crate\"]` is not a \
         suffix of the deep real module and no re-export proves it; the deleted `|| prefix` arm \
         would have resolved it. got {via_crate_ancestor:?}"
    );

    // RED (boundary effect): a sink fn taking the FORGED-qualifier token and
    // returning a bearing output. Pre-fix the input resolved to the safe token →
    // the input was classifiable AND the forgeable detection was skipped, so NO
    // check fired. Post-fix the input is `Unresolved` (an unread PascalCase
    // non-container / non-token / non-non-authority input) → the input
    // completeness check FIRES. (`AdmittedPublishedMember` is NOT a
    // KNOWN_NON_AUTHORITY external, so an Unresolved one is genuinely
    // unclassifiable.)
    let bearing: std::collections::BTreeSet<TypeDefId> =
        [synthetic_id("ExpandedField")].into_iter().collect();
    let forged_input_sig = SinkFnSig {
        module_path: "crate::meta_resolve::projectors::props".to_string(),
        name: "leak_forged_token".to_string(),
        input_idents: [forged.clone()].into_iter().collect(),
        output_idents: [TypeRef::Resolved(synthetic_id("ExpandedField"))]
            .into_iter()
            .collect(),
        mut_outparam_idents: std::collections::BTreeSet::new(),
        test_gated: false,
        module_private: false,
    };
    let unclassifiable = unclassifiable_input_idents(&[forged_input_sig], &bearing, &name_to_ids);
    assert!(
        unclassifiable
            .iter()
            .any(|(f, id)| f.contains("leak_forged_token") && id == "AdmittedPublishedMember"),
        "self-test: a sink fn taking a FORGED-qualifier `external::AdmittedPublishedMember` (now \
         Unresolved) and returning a bearing output MUST FIRE the input completeness check \
         fail-closed (it is no longer blessed as a safe admitted token); got {unclassifiable:?}"
    );

    // GREEN (boundary effect): a sink fn taking the GENUINELY-qualified token and
    // returning a bearing output is CLASSIFIABLE (the input resolved to the safe
    // token) → the input completeness check does NOT fire on it.
    let genuine_input_sig = SinkFnSig {
        module_path: "crate::meta_resolve::projectors::props".to_string(),
        name: "ok_genuine_token".to_string(),
        input_idents: [genuine.clone()].into_iter().collect(),
        output_idents: [TypeRef::Resolved(synthetic_id("ExpandedField"))]
            .into_iter()
            .collect(),
        mut_outparam_idents: std::collections::BTreeSet::new(),
        test_gated: false,
        module_private: false,
    };
    assert!(
        unclassifiable_input_idents(&[genuine_input_sig], &bearing, &name_to_ids).is_empty(),
        "self-test: a sink fn taking the GENUINELY-qualified token (resolved to the safe token) \
         must NOT fire the input completeness check"
    );
}

#[test]
fn super_qualifier_cannot_escape_crate_root_self_test_discriminates() {
    // A `super` may pop a child segment but MUST NOT pop the final / `crate`
    // segment — you cannot go ABOVE the crate root. `normalize_relative_qualifier`
    // fails closed (returns `None`) the moment a `super` would pop the last
    // segment. Pre-fix it only failed `if base.is_empty()` — one pop too late: a
    // `super::X` from module `crate` popped `["crate"]` to `[]` then extended to a
    // bare `["X"]` that matched loosely (the RED below FAILS pre-fix because it
    // returns `Some(["X"])` instead of `None`).
    let from = |q: &[&str], m: &str| {
        let qual: Vec<String> = q.iter().map(|s| s.to_string()).collect();
        normalize_relative_qualifier(&qual, m)
    };

    // RED (escape attempt): `super::X` from the crate root `crate` MUST fail
    // closed.
    assert_eq!(
        from(&["super", "X"], "crate"),
        None,
        "self-test: `super::X` from module `crate` MUST be `None` (a `super` cannot escape above \
         the crate root) — pre-fix it normalized to the loosely-matching bare `[\"X\"]`"
    );
    // RED (over-deep chain): two `super`s from a one-deep `crate::child` also
    // escape — `None`.
    assert_eq!(
        from(&["super", "super", "X"], "crate::child"),
        None,
        "self-test: `super::super::X` from a one-deep `crate::child` MUST be `None` (the second \
         `super` would pop the `crate` root)"
    );

    // GREEN (legitimate): `super::X` from `crate::child` pops `child`, rebasing to
    // `["crate","X"]`.
    assert_eq!(
        from(&["super", "X"], "crate::child"),
        Some(vec!["crate".to_string(), "X".to_string()]),
        "self-test: `super::X` from `crate::child` MUST resolve to `[\"crate\",\"X\"]`"
    );
    // GREEN (legitimate deeper): `super::super::X` from `crate::a::b` pops `b`
    // then `a`, rebasing to `["crate","X"]`.
    assert_eq!(
        from(&["super", "super", "X"], "crate::a::b"),
        Some(vec!["crate".to_string(), "X".to_string()]),
        "self-test: `super::super::X` from `crate::a::b` MUST resolve to `[\"crate\",\"X\"]`"
    );
}

#[test]
fn reexport_index_visibility_restricted_to_pub_and_pub_crate_self_test_discriminates() {
    // The re-export prover may trust ONLY a GENUINE crate-wide re-export — `pub`
    // or `pub(crate)` — as proof of identity. A NARROW `pub(self)` / `pub(super)`
    // / `pub(in some::scope)` re-export is visible to a SMALLER region than the
    // crate, so it does NOT create a re-export path an arbitrary other module can
    // write; recording it as crate-wide proof is fail-OPEN. A private `use …`
    // (Inherited) is likewise not a re-export.
    //
    // This discriminates `use_is_reexport`: with the prior over-broad
    // `matches!(Public(_) | Restricted(_))` predicate, `pub(self)`, `pub(super)`,
    // and `pub(in …)` ALL return `true` (recorded as re-exports) → the
    // narrow-visibility assertions below FAIL. With the restriction to `Public` +
    // a `Restricted` whose path is EXACTLY `crate`, only `pub` / `pub(crate)`
    // return `true`.
    let parse_vis = |src: &str| -> syn::Visibility {
        // Parse the visibility off a `<vis> use x::Y;` item so the test exercises
        // the EXACT `syn::Visibility` the index builder sees.
        let item: syn::ItemUse = syn::parse_str(src).expect("parse use item");
        item.vis
    };

    // GENUINE crate-wide re-exports — recorded.
    assert!(
        use_is_reexport(&parse_vis("pub use crate::real::Foo as Bar;")),
        "self-test: `pub use` MUST be recorded as a genuine re-export"
    );
    assert!(
        use_is_reexport(&parse_vis("pub(crate) use crate::real::Foo as Bar;")),
        "self-test: `pub(crate) use` MUST be recorded as a genuine crate-wide re-export"
    );

    // NARROW / private — NOT recorded (the fail-OPEN the fix closes).
    assert!(
        !use_is_reexport(&parse_vis("pub(self) use crate::real::Foo as Bar;")),
        "self-test: `pub(self) use` is visible to a SMALLER region than the crate and MUST NOT be \
         recorded as a re-export — trusting it as crate-wide proof is the fail-OPEN this closes"
    );
    assert!(
        !use_is_reexport(&parse_vis("pub(super) use crate::real::Foo as Bar;")),
        "self-test: `pub(super) use` is narrower than crate-wide and MUST NOT be recorded as a \
         re-export"
    );
    assert!(
        !use_is_reexport(&parse_vis(
            "pub(in crate::some::scope) use crate::real::Foo as Bar;"
        )),
        "self-test: `pub(in path) use` is a narrow scoped re-export and MUST NOT be recorded as a \
         crate-wide re-export"
    );
    assert!(
        !use_is_reexport(&parse_vis("use crate::real::Foo as Bar;")),
        "self-test: a private `use` (Inherited) is not a re-export and MUST NOT be recorded"
    );

    // END-TO-END (resolver level): an ancestor-shortened ref whose ONLY would-be
    // proof would be a NARROW (rejected) re-export stays `Unresolved`. The token's
    // real home is `crate::meta_resolve::projectors::publication_authority`; the
    // written qualifier `crate::meta_resolve::projectors` is an ANCESTOR PREFIX
    // (post-fix it cannot direct-match), so it resolves ONLY via the re-export
    // rail. A re-export index where the only entry for the id came from a
    // `pub(self) use` would be EMPTY (the narrow `use` is rejected by
    // `use_is_reexport`), so the ref stays `Unresolved` and the boundary fires
    // fail-closed. (With the over-broad predicate the narrow `use` would be
    // recorded and this would resolve — the fail-OPEN.)
    let token = TypeDefId::new(
        "crate::meta_resolve::projectors::publication_authority",
        "AdmittedPublishedMember",
    );
    let mut name_to_ids: NameDefIndex = BTreeMap::new();
    name_to_ids
        .entry("AdmittedPublishedMember".to_string())
        .or_default()
        .insert(token.clone());
    let resolve_index = name_index_with_seed_ids(&name_to_ids);
    let via_rejected_narrow = resolve_type_ref(
        &[
            "crate".to_string(),
            "meta_resolve".to_string(),
            "projectors".to_string(),
            "AdmittedPublishedMember".to_string(),
        ],
        "crate::meta_resolve::projectors::props",
        &UseIndex::default(),
        &resolve_index,
        // EMPTY — modelling a re-export index where the only candidate proof was a
        // narrow `pub(self)`/`pub(in …)` `use` that `use_is_reexport` rejected.
        &ReExportIndex::default(),
        &UseBindingIndex::default(),
    );
    assert!(
        via_rejected_narrow.resolved().is_none(),
        "self-test: an ancestor-shortened ref whose only would-be proof is a NARROW (rejected) \
         re-export MUST stay Unresolved — the narrow `use` is not recorded, so the prover has no \
         entry; got {via_rejected_narrow:?}"
    );
}

#[test]
fn import_shadow_to_unresolvable_target_stays_unresolved_self_test_discriminates() {
    // The import-shadow fail-closed property + the genuine private use-binding
    // CHAIN. RED: a UNIQUE `use external::Whatever as AdmittedPublishedMember`
    // import names an EXTERNAL/unprovable target, so the local
    // `AdmittedPublishedMember` is NOT the unique sanctioned-token def — it MUST
    // stay `Unresolved` and the sink boundary FIRES. Pre-fix the import recursion
    // returned `Unresolved` and the code fell through to the `candidates.len() ==
    // 1` shortcut, resolving `AdmittedPublishedMember` to the sanctioned token by
    // uniqueness (the fail-OPEN — both forgeable-input and unclassifiable-input
    // detections were then skipped); the carve is now DELETED, so an import that
    // claims the name and fails to resolve-by-proof stays `Unresolved`. GREEN: the
    // genuine `registry_decl` chain — a child module's `use super::X` whose target
    // resolves through the PARENT's private `use super::declaration_metadata::X`
    // binding — STILL resolves, BY PROOF (the use-binding rail), to the real home.
    let token = TypeDefId::new(
        "crate::meta_resolve::projectors::publication_authority",
        "AdmittedPublishedMember",
    );
    let mut name_to_ids: NameDefIndex = BTreeMap::new();
    name_to_ids
        .entry("AdmittedPublishedMember".to_string())
        .or_default()
        .insert(token.clone());
    let resolve_index = name_index_with_seed_ids(&name_to_ids);

    // The shadowing import: `use external::Whatever as AdmittedPublishedMember;`
    // (a RENAME — its target final segment `Whatever` differs from the local
    // name). Build a `UseIndex` carrying exactly that alias.
    let mut uses = UseIndex::default();
    uses.add(
        "AdmittedPublishedMember".to_string(),
        vec!["external".to_string(), "Whatever".to_string()],
    );

    // RED (resolver level): the bare `AdmittedPublishedMember` MUST stay Unresolved
    // (the unique import shadows it onto an unresolvable external target). Pre-fix
    // it resolved to the token by the unique-collected-def fall-through.
    let shadowed = resolve_type_ref(
        &["AdmittedPublishedMember".to_string()],
        "crate::meta_resolve::projectors::props",
        &uses,
        &resolve_index,
        &ReExportIndex::default(),
        &UseBindingIndex::default(),
    );
    assert!(
        shadowed.resolved().is_none(),
        "self-test: a bare `AdmittedPublishedMember` shadowed by `use external::Whatever as \
         AdmittedPublishedMember` MUST stay Unresolved (the import names an external/unresolvable \
         target, so the name is NOT the unique sanctioned-token def) — resolving it by uniqueness \
         is the fail-OPEN; got {shadowed:?}"
    );

    // RED (boundary effect): a sink fn taking the shadowed (now Unresolved) input
    // and returning a bearing output MUST FIRE the input completeness check
    // fail-closed (it is no longer blessed as a safe admitted token).
    let bearing: std::collections::BTreeSet<TypeDefId> =
        [synthetic_id("ExpandedField")].into_iter().collect();
    let leak_sig = SinkFnSig {
        module_path: "crate::meta_resolve::projectors::props".to_string(),
        name: "leak_shadowed_token".to_string(),
        input_idents: [shadowed.clone()].into_iter().collect(),
        output_idents: [TypeRef::Resolved(synthetic_id("ExpandedField"))]
            .into_iter()
            .collect(),
        mut_outparam_idents: std::collections::BTreeSet::new(),
        test_gated: false,
        module_private: false,
    };
    // The preserved Unresolved ref carries the IMPORT TARGET path
    // (`external::Whatever`), so its final segment is `Whatever` (the import is a
    // rename). The boundary STILL FIRES fail-closed — on the target ident.
    let unclassifiable = unclassifiable_input_idents(&[leak_sig], &bearing, &name_to_ids);
    assert!(
        unclassifiable
            .iter()
            .any(|(f, id)| f.contains("leak_shadowed_token") && id == "Whatever"),
        "self-test: a sink fn taking the shadowed (Unresolved) `AdmittedPublishedMember` and \
         returning a bearing output MUST FIRE the input completeness check fail-closed (on the \
         preserved external import-target ident `Whatever`); got {unclassifiable:?}"
    );

    // GREEN (the genuine private use-binding CHAIN — the real `registry_decl`
    // case in miniature): a bare `ResolvedTypeDeclaration` reference in a CHILD
    // module (`…::component_meta_query_engine::registry_decl`) whose file import is
    // `use super::ResolvedTypeDeclaration` (target = the PARENT module). The parent
    // module (`…::component_meta_query_engine`) carries a PRIVATE
    // `use super::declaration_metadata::ResolvedTypeDeclaration` (recorded in the
    // use-binding index, `Inherited`), and the real def home is
    // `crate::resolver_core::declaration_metadata::ResolvedTypeDeclaration`. The
    // reference resolves to that EXACT home BY PROOF — file import → qualified
    // import target at the parent → the parent's descendant-visible private
    // use-binding → DIRECT match at the real home. NOT a uniqueness fall-through:
    // this is the genuine chain the resolver MUST keep resolving.
    let parent = "crate::resolver_core::component_meta_query_engine";
    let child = "crate::resolver_core::component_meta_query_engine::registry_decl";
    let real_home = TypeDefId::new(
        "crate::resolver_core::declaration_metadata",
        "ResolvedTypeDeclaration",
    );
    // Name index: ONLY the real struct home (a UNIQUE name — so a uniqueness
    // shortcut, if it still existed, could resolve a forged ref to it; this
    // GREEN proves the GENUINE chain resolves by PROOF, and the companion #1 RED
    // proves a forged ref over the same unique name does NOT).
    let mut chain_index: NameDefIndex = BTreeMap::new();
    chain_index
        .entry("ResolvedTypeDeclaration".to_string())
        .or_default()
        .insert(real_home.clone());
    let chain_index = name_index_with_seed_ids(&chain_index);
    // The child's file import: `use super::ResolvedTypeDeclaration`.
    let mut child_uses = UseIndex::default();
    child_uses.add(
        "ResolvedTypeDeclaration".to_string(),
        vec!["super".to_string(), "ResolvedTypeDeclaration".to_string()],
    );
    // The parent's PRIVATE use-binding: `use super::declaration_metadata::
    // ResolvedTypeDeclaration` (Inherited), keyed `(parent_module, name)`.
    let mut chain_bindings: UseBindingIndex = BTreeMap::new();
    chain_bindings.insert(
        (parent.to_string(), "ResolvedTypeDeclaration".to_string()),
        vec![UseBindingTarget {
            target_path: vec![
                "super".to_string(),
                "declaration_metadata".to_string(),
                "ResolvedTypeDeclaration".to_string(),
            ],
            visibility: UseVisibility::Inherited,
        }],
    );
    let chained = resolve_type_ref(
        &["ResolvedTypeDeclaration".to_string()],
        child,
        &child_uses,
        &chain_index,
        &ReExportIndex::default(),
        &chain_bindings,
    );
    assert_eq!(
        chained.resolved(),
        Some(&real_home),
        "self-test (#1 GREEN, genuine private chain): a bare `ResolvedTypeDeclaration` in a child \
         module whose `use super::ResolvedTypeDeclaration` lands on a parent carrying a PRIVATE \
         `use super::declaration_metadata::ResolvedTypeDeclaration` MUST resolve to the real home \
         `crate::resolver_core::declaration_metadata::ResolvedTypeDeclaration` BY PROOF (the \
         use-binding chain), NOT Unresolved; got {chained:?}"
    );
    // RED (the use-binding rail is genuinely LOAD-BEARING): the SAME chain with an
    // EMPTY use-binding index MUST stay `Unresolved` — the parent's private
    // binding is the only proof that the import target (`…::component_meta_query_engine::
    // ResolvedTypeDeclaration`, which names NO def there) reaches the real home.
    // (If this resolved with no bindings, the chain would be resolving by some
    // residual permissiveness, not the proof rail.)
    let chained_no_rail = resolve_type_ref(
        &["ResolvedTypeDeclaration".to_string()],
        child,
        &child_uses,
        &chain_index,
        &ReExportIndex::default(),
        &UseBindingIndex::default(),
    );
    assert!(
        chained_no_rail.resolved().is_none(),
        "self-test (#1 GREEN, rail load-bearing): with an EMPTY use-binding index the same \
         `super::ResolvedTypeDeclaration` import target names no def at the parent and has no \
         proof, so it MUST stay Unresolved — the use-binding rail is the only thing that resolves \
         the genuine chain; got {chained_no_rail:?}"
    );
}

#[test]
fn forged_intra_crate_import_over_unique_def_stays_unresolved_self_test_discriminates() {
    // The forged-carve exploit (#1 RED): a `use crate::evil::AdmittedPublishedMember;`
    // import is INTRA-CRATE-rooted and NON-RENAMED (its final segment EQUALS the
    // local name) over a UNIQUE sanctioned token — exactly the shape the deleted
    // `intra_crate && same_name && candidates.len() == 1` carve resolved by
    // uniqueness. But the written target `crate::evil::AdmittedPublishedMember`
    // names NO real def-home (`crate::evil` defines nothing), has NO `pub`/
    // `pub(crate)` re-export, and NO accessible private use-binding chain — so the
    // import CLAIMS the local name and fails to resolve-by-proof. It MUST stay
    // `Unresolved` (NEVER blessed by the unique real token), and a sink fn taking
    // it FIRES fail-closed. Pre-fix the carve resolved the bare name to the token
    // by uniqueness and the input was exempted (no fire).
    let token = TypeDefId::new(
        "crate::meta_resolve::projectors::publication_authority",
        "AdmittedPublishedMember",
    );
    let mut name_to_ids: NameDefIndex = BTreeMap::new();
    name_to_ids
        .entry("AdmittedPublishedMember".to_string())
        .or_default()
        .insert(token.clone());
    let resolve_index = name_index_with_seed_ids(&name_to_ids);

    // The forged import: `use crate::evil::AdmittedPublishedMember;` (intra-crate,
    // NON-renamed). `crate::evil` is not a collected def-home for this name.
    let mut uses = UseIndex::default();
    uses.add(
        "AdmittedPublishedMember".to_string(),
        vec![
            "crate".to_string(),
            "evil".to_string(),
            "AdmittedPublishedMember".to_string(),
        ],
    );

    // RED (resolver level): the bare `AdmittedPublishedMember` MUST stay
    // Unresolved (the intra-crate import claims the name but its target
    // `crate::evil::AdmittedPublishedMember` resolves by NO proof — no def-home, no
    // re-export, no binding). Pre-fix the `intra_crate && same_name &&
    // candidates.len() == 1` carve resolved it to the token by uniqueness.
    let forged = resolve_type_ref(
        &["AdmittedPublishedMember".to_string()],
        "crate::meta_resolve::projectors::props",
        &uses,
        &resolve_index,
        &ReExportIndex::default(),
        &UseBindingIndex::default(),
    );
    assert!(
        forged.resolved().is_none(),
        "self-test (#1 RED, forged carve): a `use crate::evil::AdmittedPublishedMember` import \
         (intra-crate, non-renamed) over the UNIQUE token MUST stay Unresolved — its target \
         `crate::evil::AdmittedPublishedMember` resolves by no proof (no def-home, no re-export, \
         no binding); the deleted uniqueness carve resolved it. got {forged:?}"
    );

    // RED (boundary effect): a sink fn taking the forged (now Unresolved) input
    // and returning a bearing output MUST FIRE the input completeness check
    // fail-closed (it is no longer blessed as a safe admitted token). The
    // preserved Unresolved ref carries the IMPORT TARGET path, whose final segment
    // is the token name (a non-renamed import) — the boundary fires on it.
    let bearing: std::collections::BTreeSet<TypeDefId> =
        [synthetic_id("ExpandedField")].into_iter().collect();
    let leak_sig = SinkFnSig {
        module_path: "crate::meta_resolve::projectors::props".to_string(),
        name: "leak_forged_carve_token".to_string(),
        input_idents: [forged.clone()].into_iter().collect(),
        output_idents: [TypeRef::Resolved(synthetic_id("ExpandedField"))]
            .into_iter()
            .collect(),
        mut_outparam_idents: std::collections::BTreeSet::new(),
        test_gated: false,
        module_private: false,
    };
    let unclassifiable = unclassifiable_input_idents(&[leak_sig], &bearing, &name_to_ids);
    assert!(
        unclassifiable
            .iter()
            .any(|(f, id)| f.contains("leak_forged_carve_token") && id == "AdmittedPublishedMember"),
        "self-test (#1 RED, boundary): a sink fn taking the forged (Unresolved) \
         `AdmittedPublishedMember` and returning a bearing output MUST FIRE the input completeness \
         check fail-closed; got {unclassifiable:?}"
    );
}

#[test]
fn reexport_prover_requires_exact_target_module_not_suffix_self_test_discriminates() {
    // The re-export-target suffix-slack exploit (#2 RED): a candidate whose real
    // home is `crate::a::projectors::publication_authority`, plus a `pub` re-export
    // (written in an UNRELATED module `crate::wrong`) whose TARGET qualifier is the
    // SINGLE segment `publication_authority` — which suffix-matches the candidate's
    // last module segment but is NOT the candidate's real home. SUFFIX slack on the
    // re-export TARGET would prove identity just because the last segment matches;
    // the EXACT-target rule does NOT (neither the absolute `publication_authority`
    // nor the child-relative `crate::wrong::publication_authority` equals the real
    // home). The written ref through the re-exporting module MUST stay `Unresolved`.
    let candidate = TypeDefId::new(
        "crate::a::projectors::publication_authority",
        "AdmittedPublishedMember",
    );
    let mut name_to_ids: NameDefIndex = BTreeMap::new();
    name_to_ids
        .entry("AdmittedPublishedMember".to_string())
        .or_default()
        .insert(candidate.clone());
    let resolve_index = name_index_with_seed_ids(&name_to_ids);

    // The re-exporting module `crate::wrong` declares
    // `pub use publication_authority::AdmittedPublishedMember;` — a re-export whose
    // TARGET qualifier `publication_authority` (single segment) is a SUFFIX match of
    // the candidate's last module segment but whose absolute / child-relative
    // interpretations (`publication_authority` / `crate::wrong::publication_authority`)
    // are NOT the candidate's real home `crate::a::projectors::publication_authority`.
    let mut reexports = ReExportIndex::default();
    reexports
        .entry(TypeDefId::new("crate::wrong", "AdmittedPublishedMember"))
        .or_default()
        .insert(vec![
            "publication_authority".to_string(),
            "AdmittedPublishedMember".to_string(),
        ]);

    // RED: the written ref `crate::wrong::AdmittedPublishedMember` MUST stay
    // Unresolved — its re-export targets a `publication_authority` qualifier whose
    // exact module interpretations are NOT the candidate's real home (only a
    // single-segment SUFFIX of it). Pre-fix the suffix-or-equal target match proved
    // it.
    let via_suffix_slack = resolve_type_ref(
        &[
            "crate".to_string(),
            "wrong".to_string(),
            "AdmittedPublishedMember".to_string(),
        ],
        "crate::wrong::props",
        &UseIndex::default(),
        &resolve_index,
        &reexports,
        &UseBindingIndex::default(),
    );
    assert!(
        via_suffix_slack.resolved().is_none(),
        "self-test (#2 RED, re-export-target suffix-slack): a re-export whose TARGET qualifier \
         `publication_authority` merely SUFFIX-matches the candidate's last module segment MUST \
         NOT prove the candidate at `crate::a::projectors::publication_authority` — the target \
         match is EXACT, never suffix; got {via_suffix_slack:?}"
    );

    // GREEN (the EXACT-target rule resolves a genuine re-export): a re-export whose
    // TARGET module IS the candidate's real home EXACTLY resolves. The re-exporting
    // module `crate::a::projectors` declares `pub use publication_authority::
    // AdmittedPublishedMember;` — its EXACT child-relative interpretation
    // `crate::a::projectors::publication_authority` equals the real home. Proves the
    // EXACT rule is not vacuously rejecting everything.
    let mut exact_reexports = ReExportIndex::default();
    exact_reexports
        .entry(TypeDefId::new(
            "crate::a::projectors",
            "AdmittedPublishedMember",
        ))
        .or_default()
        .insert(vec![
            "publication_authority".to_string(),
            "AdmittedPublishedMember".to_string(),
        ]);
    let via_exact = resolve_type_ref(
        &[
            "crate".to_string(),
            "a".to_string(),
            "projectors".to_string(),
            "AdmittedPublishedMember".to_string(),
        ],
        "crate::a::projectors::props",
        &UseIndex::default(),
        &resolve_index,
        &exact_reexports,
        &UseBindingIndex::default(),
    );
    assert_eq!(
        via_exact.resolved(),
        Some(&candidate),
        "self-test (#2 GREEN, exact target): a re-export whose EXACT child-relative target module \
         is the candidate's real home `crate::a::projectors::publication_authority` MUST resolve \
         the ancestor-shortened ref; got {via_exact:?}"
    );
}

#[test]
fn unrooted_qualifier_shadowed_rebinds_but_unshadowed_suffix_match_is_characterized() {
    // CHARACTERIZATION (not a complete-proof claim): this pins TWO behaviors of
    // the unrooted-qualifier arm — the shadowed REBIND (proof; RED below) and the
    // unshadowed RAW-SUFFIX match (ACCEPTED RESIDUAL; GREEN below, tracked in
    // docs/arch/output-projector-residual-guard-debt.md).
    //
    // The shadowed rebind (#3 RED): a file `use`-SHADOWS the first segment of an
    // UNROOTED qualifier (`use crate::other as publication_authority;`) then
    // writes `publication_authority::AdmittedPublishedMember`. The raw suffix
    // match would resolve the unrooted `publication_authority` qualifier to the
    // safe token at `crate::meta_resolve::projectors::publication_authority` — but
    // the file's `use` binds `publication_authority` to a DIFFERENT module, so the
    // qualifier does NOT name the token's module. It MUST stay `Unresolved`.
    let token = TypeDefId::new(
        "crate::meta_resolve::projectors::publication_authority",
        "AdmittedPublishedMember",
    );
    let mut name_to_ids: NameDefIndex = BTreeMap::new();
    name_to_ids
        .entry("AdmittedPublishedMember".to_string())
        .or_default()
        .insert(token.clone());
    let resolve_index = name_index_with_seed_ids(&name_to_ids);

    // The shadowing import: `use crate::other as publication_authority;` — binds
    // the module alias `publication_authority` to `crate::other` (a DIFFERENT
    // module that defines no `AdmittedPublishedMember`).
    let mut uses = UseIndex::default();
    uses.add(
        "publication_authority".to_string(),
        vec!["crate".to_string(), "other".to_string()],
    );

    // RED: `publication_authority::AdmittedPublishedMember` MUST stay Unresolved —
    // the unrooted first segment `publication_authority` is `use`-shadowed to
    // `crate::other`, so the qualifier names `crate::other::AdmittedPublishedMember`
    // (which resolves by no proof), NOT the token's real module. Pre-fix the raw
    // suffix match resolved it to the token.
    let shadowed = resolve_type_ref(
        &[
            "publication_authority".to_string(),
            "AdmittedPublishedMember".to_string(),
        ],
        "crate::meta_resolve::projectors::props",
        &uses,
        &resolve_index,
        &ReExportIndex::default(),
        &UseBindingIndex::default(),
    );
    assert!(
        shadowed.resolved().is_none(),
        "self-test (#3 RED, unrooted-qualifier shadow): a `use crate::other as \
         publication_authority` then `publication_authority::AdmittedPublishedMember` MUST stay \
         Unresolved — the unrooted first segment is `use`-shadowed away from the token's real \
         module; the raw suffix match resolved it pre-fix. got {shadowed:?}"
    );

    // GREEN — ACCEPTED RESIDUAL (characterization, NOT proof): with NO shadowing
    // import, a written `publication_authority::AdmittedPublishedMember` whose
    // suffix matches the token's real module resolves by RAW SUFFIX. This is the
    // unrooted-unshadowed residual (debt-tracked): it is not proof the qualifier
    // genuinely names the token's module (an unrooted first segment is a genuine
    // module segment for the extern-crate-name / sibling-module case the real tree
    // relies on, but is also where a forged unrooted decoy would land). It is
    // accepted because the token is uniquely named (the match lands on the single
    // genuine def — no same-named decoy to forge) and the compiler-enforced
    // sealed-token boundary is the production guarantee. This case pins that the
    // shadow rule rejects the shadowed case while the unshadowed raw-suffix match
    // is the characterized accepted behavior — it is NOT a claim of complete Rust
    // name resolution.
    let genuine = resolve_type_ref(
        &[
            "publication_authority".to_string(),
            "AdmittedPublishedMember".to_string(),
        ],
        "crate::meta_resolve::projectors::props",
        &UseIndex::default(),
        &resolve_index,
        &ReExportIndex::default(),
        &UseBindingIndex::default(),
    );
    assert_eq!(
        genuine.resolved(),
        Some(&token),
        "self-test (#3 GREEN, unshadowed unrooted — ACCEPTED RESIDUAL): an UNSHADOWED \
         `publication_authority::AdmittedPublishedMember` whose suffix matches the token's real \
         module raw-suffix resolves (characterized accepted behavior, not complete-proof; only a \
         `use`-shadowed first segment is rebound); got {genuine:?}"
    );
}

#[test]
fn dual_bearing_input_stays_forgeable_self_test_discriminates() {
    // GAP-2: the bearing-output-skip fence is TRANSITIVE-aware via the
    // direct-resolution-authority carve-out. A DUAL type that BOTH bears
    // `TypeExpr` AND directly co-holds a resolution-authority seed must STAY
    // forgeable (the fence must NOT silently drop it just because it is also a
    // bearing output); a pure published DTO (bearing, no forgeable seed) must NOT
    // be forgeable; a resolution-authority wrapper that reaches `TypeExpr` only
    // through a NESTED sub-struct (a direct `SurfaceView` field + a separate
    // bearing field) must also stay forgeable.
    // Built DIRECTLY with explicit qualified ids so the bearing `IndexSignature`
    // is genuinely `verter_type_expr::IndexSignature` (TypeExpr fields), DISTINCT
    // from the authority `crate::semantic_query::IndexSignature` input seed.
    let m = "crate::test_synthetic";
    let mk_def = |refs: &[TypeRef]| TypeDefRefs {
        refs: refs.iter().cloned().collect(),
    };
    let defs: BTreeMap<TypeDefId, TypeDefRefs> = [
        // A DUAL type: directly co-holds `SurfaceMember` (RA-seed) AND `TypeExpr`.
        (
            TypeDefId::new(m, "Dual"),
            mk_def(&[
                resolved_ref("crate::semantic_query", "SurfaceMember"),
                resolved_ref("verter_type_expr", "TypeExpr"),
            ]),
        ),
        // A bearing sub-struct (`FunctionExpr`) reaching `TypeExpr`.
        (
            TypeDefId::new(m, "FunctionExpr"),
            mk_def(&[resolved_ref("verter_type_expr", "TypeExpr")]),
        ),
        // A resolution-authority wrapper: directly holds `SurfaceView`, reaches
        // `TypeExpr` only through the nested bearing `FunctionExpr`.
        (
            TypeDefId::new(m, "RaWrapper"),
            mk_def(&[
                resolved_ref("crate::semantic_query", "SurfaceView"),
                resolved_ref(m, "FunctionExpr"),
            ]),
        ),
        // A PURE published DTO: bearing, but holds NO forgeable seed.
        (
            TypeDefId::new(m, "PureDto"),
            mk_def(&[resolved_ref("verter_type_expr", "TypeExpr")]),
        ),
        // An ALREADY-LOWERED-IR holder: directly holds the BEARING
        // `verter_type_expr::IndexSignature` (itself part of TypeExpr's structure,
        // NOT an authority seed), plus `TypeExpr`. It is NOT a resolution-authority
        // wrapper, so it must NOT be carved into the forgeable set by GAP-2.
        (
            TypeDefId::new(m, "IrHolder"),
            mk_def(&[
                resolved_ref("verter_type_expr", "IndexSignature"),
                resolved_ref("verter_type_expr", "TypeExpr"),
            ]),
        ),
    ]
    .into_iter()
    .collect();
    let bearing = typeexpr_bearing_closure(&defs);
    let forgeable = forgeable_authority_closure(&defs, &bearing);

    // Sanity: all four are bearing (each reaches `TypeExpr`).
    for name in ["Dual", "RaWrapper", "PureDto", "IrHolder"] {
        assert!(
            set_contains_name(&bearing, name),
            "self-test setup: `{name}` must be TypeExpr-bearing; got {bearing:?}"
        );
    }
    // The DUAL type and the RA-wrapper STAY forgeable (GAP-2 carve-out).
    assert!(
        set_contains_name(&forgeable, "Dual"),
        "self-test: a DUAL type `Dual {{ m: SurfaceMember, t: TypeExpr }}` (bearing AND directly \
         co-holding a resolution-authority seed) MUST STAY forgeable — the fence must not drop it; \
         got {forgeable:?}"
    );
    assert!(
        set_contains_name(&forgeable, "RaWrapper"),
        "self-test: a resolution-authority wrapper `RaWrapper {{ v: SurfaceView, h: FunctionExpr }}` \
         (directly holds an RA-seed, reaches `TypeExpr` through a nested sub-struct) MUST STAY \
         forgeable; got {forgeable:?}"
    );
    // The PURE DTO is NOT forgeable (no forgeable seed).
    assert!(
        !set_contains_name(&forgeable, "PureDto"),
        "self-test: a pure published DTO `PureDto {{ t: TypeExpr }}` (bearing, NO forgeable seed) \
         must NOT be forgeable — the fence correctly skips it; got {forgeable:?}"
    );
    // The already-lowered-IR holder is NOT carved in (its `IndexSignature` is the
    // type-expr one — itself bearing, TypeExpr's own structure, not authority).
    assert!(
        !set_contains_name(&forgeable, "IrHolder"),
        "self-test: a type holding the already-lowered-IR `verter_type_expr::IndexSignature` \
         (itself bearing — part of `TypeExpr`'s structure) must NOT be carved into the forgeable \
         set (else every `TypeExpr`-structure holder would wrongly fire); got {forgeable:?}"
    );

    // The boundary check fires on a sink fn taking the DUAL type and returning a
    // bearing value; it does NOT fire on a fn taking only the pure DTO. The DUAL /
    // PURE inputs resolve through a fixture index; the output `ExpandedField`
    // resolves via the seed merge to bearing.
    let mut fixture: NameDefIndex = BTreeMap::new();
    for name in ["Dual", "PureDto"] {
        fixture
            .entry(name.to_string())
            .or_default()
            .insert(TypeDefId::new(m, name));
    }
    let dual_red = synthetic_sig(
        &fixture,
        "crate::meta_resolve::projectors::output_sink",
        "dual_to_field",
        &["Dual"],
        &["ExpandedField"],
        &[],
    );
    let v = cross_sink_raw_authority_violations(&[dual_red], &bearing, &forgeable);
    assert!(
        v.iter().any(|m| m.contains("dual_to_field")),
        "self-test: a sink fn taking a DUAL type (`Dual`) and returning a bearing value MUST FIRE \
         (the fence kept `Dual` forgeable — GAP-2); got {v:?}"
    );
    let pure_green = synthetic_sig(
        &fixture,
        "crate::meta_resolve::projectors::output_sink",
        "pure_dto_to_field",
        &["PureDto"],
        &["ExpandedField"],
        &[],
    );
    let v = cross_sink_raw_authority_violations(&[pure_green], &bearing, &forgeable);
    assert!(
        v.is_empty(),
        "self-test: a sink fn taking ONLY a pure published DTO (`PureDto`, no forgeable seed) and \
         returning a bearing value must NOT fire; got {v:?}"
    );
}

#[test]
fn forgeable_input_fence_has_no_dual_bearing_type() {
    // The forgeable-authority closure's bearing fence skips a TypeExpr-bearing
    // type from the forgeable INPUT set. GAP-2 made that fence TRANSITIVE-correct:
    // it now KEEPS a dual-bearing wrapper (a type that directly co-holds a
    // resolution-authority seed AND is bearing) instead of dropping it, so the
    // closure is sound regardless of whether a dual-bearing type exists. This
    // guard stays as a BELT-AND-SUSPENDERS tripwire: it asserts the stronger,
    // simpler premise that NO production type co-holds a propagating seed AND a
    // DIRECT `TypeExpr` field (the audited invariant: RA-seeds carry their member
    // values as `SemanticNodeId`, never a co-located `TypeExpr`). If that premise
    // is ever broken, this surfaces it loudly; the closure's GAP-2 carve-out also
    // keeps the type forgeable so the cross-sink guard catches a consumer too.
    let (defs, _name_to_ids) = collect_type_defs();
    let bearing = typeexpr_bearing_closure(&defs);
    // Anti-vacuity: the bearing closure actually produced a non-empty set, and the
    // real seed/DTO defs are collected (so an empty `defs` cannot vacuously pass).
    assert!(
        bearing.contains(&TypeDefId::new("verter_type_expr", "TypeExpr")),
        "anti-vacuity: bearing closure regressed (TypeExpr not flagged)"
    );
    assert!(
        defs.contains_key(&TypeDefId::new("crate::semantic_query", "SurfaceMember"))
            && defs.contains_key(&TypeDefId::new(
                "verter_semantic::analysis::type_expand::request",
                "ExpandedField"
            )),
        "anti-vacuity: the real type defs must include the seed `crate::semantic_query::SurfaceMember` \
         and a bearing DTO `…request::ExpandedField` (the collector / read roots regressed)"
    );
    let violations = dual_bearing_violations(&defs, &bearing);
    assert!(
        violations.is_empty(),
        "DUAL-BEARING type(s) found — the forgeable-authority closure's \
         skip-bearing-output-as-input fence is UNSOUND while these exist:\n{}",
        violations.join("\n")
    );
}

#[test]
fn dual_bearing_self_test_discriminates() {
    // GREEN: a pure bearing DTO that does NOT carry a forgeable seed
    // (`ExpandedField { type: TypeExpr }`); a pure forgeable wrapper that has no
    // direct `TypeExpr` field (`WrappedSurfaceMember { member: SurfaceMember }`);
    // and the sanctioned admitted token (co-holds a seed + a `TypeExpr` but
    // EXCLUDED). None is a direct-co-hold dual-bearing type.
    let (defs, _) = synthetic_defs(&[
        ("ExpandedField", &["TypeExpr"]),
        ("WrappedSurfaceMember", &["SurfaceMember"]),
        // The sanctioned admitted token co-holds a seed + a direct `TypeExpr`,
        // but is EXCLUDED — it must NOT be reported.
        ("AdmittedPublishedMember", &["SurfaceMember", "TypeExpr"]),
    ]);
    let v = dual_bearing_violations(&defs, &typeexpr_bearing_closure(&defs));
    assert!(
        v.is_empty(),
        "self-test: a pure bearing DTO + a pure forgeable wrapper + the exempt admitted token MUST \
         pass (none is a direct-co-hold dual-bearing type); got: {v:?}"
    );

    // RED: a DUAL-BEARING type `struct Dual { m: SurfaceMember, t: TypeExpr }` —
    // directly reaches a propagating forgeable seed AND has a DIRECT `TypeExpr`
    // field. MUST FIRE.
    let (dual_defs, _) = synthetic_defs(&[("Dual", &["SurfaceMember", "TypeExpr"])]);
    let v = dual_bearing_violations(&dual_defs, &typeexpr_bearing_closure(&dual_defs));
    assert!(
        v.iter().any(|m| m.contains("Dual") && m.contains("DUAL-BEARING")),
        "self-test: a dual-bearing `struct Dual {{ m: SurfaceMember, t: TypeExpr }}` MUST FIRE the \
         fence-premise check; got: {v:?}"
    );

    // RED: an `IndexSignature` seed co-held with a direct `TypeExpr` field also
    // fires (every propagating seed counts, not just SurfaceMember). `IndexSignature`
    // here resolves to the AUTHORITY `crate::semantic_query::IndexSignature` seed.
    let (dual_idx, _) = synthetic_defs(&[("DualIndex", &["IndexSignature", "TypeExpr"])]);
    let v = dual_bearing_violations(&dual_idx, &typeexpr_bearing_closure(&dual_idx));
    assert!(
        v.iter().any(|m| m.contains("DualIndex")),
        "self-test: an `IndexSignature` seed co-held with a direct `TypeExpr` field MUST FIRE; \
         got: {v:?}"
    );

    // RED (GAP-2 — TRANSITIVE seed side): a NESTED dual-bearing type — a DIRECT
    // `TypeExpr` field PLUS a TRANSITIVE reach to a resolution-authority seed
    // through a sub-wrapper (`struct Raw { member: SurfaceMember }` nested inside
    // `struct NestedDual { raw: Raw, t: TypeExpr }`) — MUST FIRE. The OLD
    // DIRECT-only seed side missed this (the seed is one hop away through `Raw`).
    let (nested_dual, _) = synthetic_defs(&[
        ("Raw", &["SurfaceMember"]),
        ("NestedDual", &["Raw", "TypeExpr"]),
    ]);
    let v = dual_bearing_violations(&nested_dual, &typeexpr_bearing_closure(&nested_dual));
    assert!(
        v.iter().any(|m| m.contains("NestedDual")),
        "self-test: a NESTED dual-bearing `struct NestedDual {{ raw: Raw, t: TypeExpr }}` (a DIRECT \
         `TypeExpr` field + a TRANSITIVE reach to `SurfaceMember` through `Raw`) MUST FIRE — the \
         tripwire seed side is TRANSITIVE (GAP-2); got: {v:?}"
    );

    // GREEN (scoping — the real-tree IR shape): a type that reaches a propagating
    // seed AND reaches `TypeExpr` only THROUGH a SEPARATE nested sub-struct (NOT a
    // direct `TypeExpr` field) — the `SemanticNodeData` / `ObjectMember` shape —
    // must NOT fire. The DIRECT `TypeExpr` field is required, and the seed member
    // value is a `SemanticNodeId`, so there is no co-located `TypeExpr`.
    let (nested_reach, _) = synthetic_defs(&[
        ("FunctionExpr", &["TypeExpr"]),
        // `enum ObjectMemberLike { Index(IndexSignature), Call(FunctionExpr) }`:
        // reaches the `IndexSignature` seed AND reaches `TypeExpr` via
        // `FunctionExpr` — but NO direct `TypeExpr` field.
        ("ObjectMemberLike", &["IndexSignature", "FunctionExpr"]),
    ]);
    let v = dual_bearing_violations(&nested_reach, &typeexpr_bearing_closure(&nested_reach));
    assert!(
        v.is_empty(),
        "self-test: a type reaching a seed and reaching `TypeExpr` only THROUGH a \
         nested sub-struct (no direct `TypeExpr` field) must NOT fire — this is the real-tree IR \
         shape (`SemanticNodeData` / `ObjectMember`); the fence stays sound for it; got: {v:?}"
    );

    // F3 — the sanctioned-carrier exemption is QUALIFIED `(module, name)`, NOT a
    // bare name. Build a WRONG-MODULE same-name `AdmittedPublishedMember` DIRECTLY
    // (an explicit `TypeDefId` at a NON-sanctioned module) co-holding a
    // `SurfaceMember` seed AND a DIRECT `TypeExpr` field. The real sanctioned token
    // lives at `crate::meta_resolve::projectors::publication_authority`; this
    // impostor lives at `crate::evil`.
    let evil_module = "crate::evil";
    let mk_def = |refs: &[TypeRef]| TypeDefRefs {
        refs: refs.iter().cloned().collect(),
    };
    let wrong_module_defs: BTreeMap<TypeDefId, TypeDefRefs> = [(
        TypeDefId::new(evil_module, "AdmittedPublishedMember"),
        mk_def(&[
            resolved_ref("crate::semantic_query", "SurfaceMember"),
            resolved_ref("verter_type_expr", "TypeExpr"),
        ]),
    )]
    .into_iter()
    .collect();
    let v = dual_bearing_violations(
        &wrong_module_defs,
        &typeexpr_bearing_closure(&wrong_module_defs),
    );
    assert!(
        v.iter().any(|m| m.contains("AdmittedPublishedMember") && m.contains("DUAL-BEARING")),
        "self-test (F3): a WRONG-MODULE same-name `crate::evil::AdmittedPublishedMember` co-holding \
         a `SurfaceMember` seed AND a DIRECT `TypeExpr` field MUST FIRE — it is NOT the real \
         sanctioned token (the exemption is QUALIFIED `(module, name)`, not a bare name); got: {v:?}"
    );

    // F3 GREEN: the SAME-shaped struct at the REAL sanctioned module
    // (`crate::meta_resolve::projectors::publication_authority`) IS the sanctioned
    // token and stays EXEMPT (does NOT fire).
    let real_module = "crate::meta_resolve::projectors::publication_authority";
    let real_token_defs: BTreeMap<TypeDefId, TypeDefRefs> = [(
        TypeDefId::new(real_module, "AdmittedPublishedMember"),
        mk_def(&[
            resolved_ref("crate::semantic_query", "SurfaceMember"),
            resolved_ref("verter_type_expr", "TypeExpr"),
        ]),
    )]
    .into_iter()
    .collect();
    let v = dual_bearing_violations(
        &real_token_defs,
        &typeexpr_bearing_closure(&real_token_defs),
    );
    assert!(
        v.is_empty(),
        "self-test (F3): the REAL sanctioned `…::publication_authority::AdmittedPublishedMember` \
         token (same shape) MUST stay EXEMPT (qualified-identity match); got: {v:?}"
    );
}

// ===========================================================================
// (D1-token) Admitted-token CONSTRUCTION discipline.
//
// The compiler-PRIMARY half of the publication-authority mechanism is the
// sealed token: private fields + a private `Seal` make forging a token a
// compile error. This guard pins the STRUCTURAL facts that keep that seal
// real, which a future edit could silently erode:
//   (a) EVERY field of every admitted-token struct stays PRIVATE (a `pub`
//       field would let a sibling struct-literal a token bypassing admission);
//   (b) the token structs carry a private `_seal` field (the unforgeable
//       marker);
//   (c) no admission / minter fn accepts a CALLER-SUPPLIED `kind:
//       PublishedSurfaceKind` BY-VALUE param — the published-surface kind is
//       DERIVED inside authority code (a caller-supplied kind is the
//       cosmetic-fence the ruling rejects). Implemented by `syn`-parsing the
//       production fns and rejecting a by-value `kind: PublishedSurfaceKind`
//       parameter; the `#[cfg(test)]` `admitted_for_test` ctor is exempt.
//   (d) no PRODUCTION minter that returns an admitted token accepts a forgeable
//       wire carrier (`VueMacroSurface` / `TypeInfoSurface`) with visibility
//       BROADER than its own terminal sink module — a minter visible beyond
//       `vue_exec` / `svelte_exec` that takes a forgeable surface is exactly
//       the cosmetic-seal forgeability the ruling rejects (a sibling forges the
//       carrier + mints). The Vue minter is `pub(in …vue_exec)`; the Svelte
//       minter is module-private.
//
// The token files scanned are the projector `publication_authority` module,
// the framework-surface `ResolvedVueSurface` carrier in `vue_exec`, and the
// `SvelteResolvedSurface` token in `svelte_exec`.
// ===========================================================================

/// The admitted-token struct names whose fields must ALL stay private.
const ADMITTED_TOKEN_STRUCTS: &[&str] = &[
    "ResolvedMacroPayload",
    "ResolvedPayloadSurface",
    "SurfaceMemberCandidate",
    "AdmittedPublishedMember",
    "ResolvedVueSurface",
    // Svelte's OWN sealed resolved-surface token (FIX-B): private fields +
    // private `_seal`, minted only inside `svelte_exec`.
    "SvelteResolvedSurface",
];

/// One observed admitted-token struct's field-privacy facts.
#[derive(Debug, Clone, PartialEq, Eq)]
struct TokenStructFacts {
    name: String,
    /// Any field whose visibility is NOT private (Inherited) — a leak.
    public_fields: Vec<String>,
    /// Whether the struct carries a private `_seal` field.
    has_private_seal: bool,
}

/// Collect the field-privacy facts of every [`ADMITTED_TOKEN_STRUCTS`] struct
/// in the given parsed file.
fn collect_token_struct_facts(file: &syn::File) -> Vec<TokenStructFacts> {
    struct V {
        out: Vec<TokenStructFacts>,
    }
    impl<'ast> syn::visit::Visit<'ast> for V {
        fn visit_item_struct(&mut self, s: &'ast syn::ItemStruct) {
            let name = s.ident.to_string();
            if ADMITTED_TOKEN_STRUCTS.contains(&name.as_str()) {
                let mut public_fields = Vec::new();
                let mut has_private_seal = false;
                for field in &s.fields {
                    let fname = field
                        .ident
                        .as_ref()
                        .map(|i| i.to_string())
                        .unwrap_or_else(|| "<tuple-field>".to_string());
                    let private = matches!(field.vis, syn::Visibility::Inherited);
                    if !private {
                        public_fields.push(fname.clone());
                    }
                    if fname == "_seal" && private {
                        has_private_seal = true;
                    }
                }
                self.out.push(TokenStructFacts {
                    name,
                    public_fields,
                    has_private_seal,
                });
            }
            syn::visit::visit_item_struct(self, s);
        }
    }
    let mut v = V { out: Vec::new() };
    syn::visit::Visit::visit_file(&mut v, file);
    v.out
}

/// The policy half over the collected token-struct facts: every token struct
/// must have ZERO public fields AND a private `_seal`.
fn token_struct_violations(facts: &[TokenStructFacts]) -> Vec<String> {
    let mut violations = Vec::new();
    for f in facts {
        if !f.public_fields.is_empty() {
            violations.push(format!(
                "admitted token `{}` has NON-PRIVATE field(s) {:?} — every token field MUST be \
                 private so the token cannot be struct-literal'd outside its admission fn",
                f.name, f.public_fields
            ));
        }
        if !f.has_private_seal {
            violations.push(format!(
                "admitted token `{}` is missing its private `_seal` field — the unforgeable seal \
                 marker that prevents external construction",
                f.name
            ));
        }
    }
    violations
}

/// The forgeable WIRE-CARRIER param types a production minter must not accept
/// with broad visibility — a `pub`/`pub(crate)`/… fn taking one of these and
/// returning an admitted token is the cosmetic seal (a sibling forges the
/// carrier + mints).
const FORGEABLE_WIRE_CARRIERS: &[&str] = &["VueMacroSurface", "TypeInfoSurface"];

/// Whether a fn signature has a BY-VALUE `kind: PublishedSurfaceKind` parameter
/// (the caller-supplied-kind bypass check (c)). A by-reference
/// `&PublishedSurfaceKind` or a different-typed param does NOT match — only a
/// by-value `PublishedSurfaceKind` (the form a caller would supply for the
/// admission fn to trust verbatim).
fn signature_has_by_value_published_surface_kind_param(sig: &syn::Signature) -> bool {
    sig.inputs.iter().any(|input| {
        let syn::FnArg::Typed(pat) = input else {
            return false;
        };
        // By-value: the param type is a plain path ending in
        // `PublishedSurfaceKind` (not a reference).
        if let syn::Type::Path(p) = pat.ty.as_ref() {
            return p
                .path
                .segments
                .last()
                .is_some_and(|s| s.ident == "PublishedSurfaceKind");
        }
        false
    })
}

/// Whether a fn's return type names an [`ADMITTED_TOKEN_STRUCTS`] token (it is a
/// minter). Matches the token name anywhere in the return type token stream
/// (`Token`, `Option<Token>`, `Self` is NOT matched — a `Self`-returning impl
/// method is covered by the per-struct field-privacy facts).
fn return_type_mints_admitted_token(sig: &syn::Signature) -> bool {
    let syn::ReturnType::Type(_, ty) = &sig.output else {
        return false;
    };
    type_idents(&ty.to_token_stream())
        .iter()
        .any(|id| ADMITTED_TOKEN_STRUCTS.contains(&id.as_str()))
}

/// Whether a fn signature takes a forgeable wire carrier
/// ([`FORGEABLE_WIRE_CARRIERS`]) by value or by reference as a parameter.
fn signature_takes_forgeable_wire_carrier(sig: &syn::Signature) -> bool {
    sig.inputs.iter().any(|input| {
        let syn::FnArg::Typed(pat) = input else {
            return false;
        };
        type_idents(&pat.ty.to_token_stream())
            .iter()
            .any(|id| FORGEABLE_WIRE_CARRIERS.contains(&id.as_str()))
    })
}

/// Whether a visibility reaches BEYOND a sink module: `pub` / `pub(crate)` /
/// `pub(super)` reach beyond; `pub(in ...)` reaches beyond UNLESS its path ends
/// in a sink-leaf segment (`vue_exec` / `svelte_exec`); `Inherited` /
/// `pub(self)` stay module-local. A minter taking a forgeable wire carrier must
/// stay within its sink, so a broad visibility here is the (d) violation.
fn minter_visibility_reaches_beyond_sink(vis: &syn::Visibility) -> bool {
    match vis {
        syn::Visibility::Inherited => false,
        syn::Visibility::Public(_) => true,
        syn::Visibility::Restricted(r) => {
            if r.path.is_ident("self") {
                return false;
            }
            // `pub(in crate::...::vue_exec)` / `...::svelte_exec` stays within
            // the sink leaf; any broader `pub(in ...)` (or `pub(crate)` /
            // `pub(super)` written as restricted) reaches beyond.
            let last = r.path.segments.last().map(|s| s.ident.to_string());
            !matches!(last.as_deref(), Some("vue_exec") | Some("svelte_exec"))
        }
    }
}

/// The (c) + (d) policy over every production fn in a scanned token/minter file.
/// `#[cfg(test)]` fns are dropped (production-only scan, so the `_for_test`
/// ctors are exempt). Returns one violation string per offending fn.
fn admission_minter_violations(rel: &str, file: &syn::File) -> Vec<String> {
    struct V {
        rel: String,
        out: Vec<String>,
    }
    impl V {
        fn check(&mut self, sig: &syn::Signature, attrs: &[syn::Attribute], vis: &syn::Visibility) {
            if fn_attrs_are_cfg_test(attrs) {
                return; // production-only scan
            }
            let name = sig.ident.to_string();
            // (c) caller-supplied-kind bypass.
            if signature_has_by_value_published_surface_kind_param(sig) {
                self.out.push(format!(
                    "{}: `{name}` accepts a BY-VALUE `kind: PublishedSurfaceKind` parameter — the \
                     published-surface kind MUST be DERIVED inside authority code \
                     (`published_surface_kind_for(macro_kind)`), never caller-supplied (a \
                     caller-supplied kind is the cosmetic fence the ruling rejects).",
                    self.rel
                ));
            }
            // (d) broad-visibility minter taking a forgeable wire carrier.
            if return_type_mints_admitted_token(sig)
                && signature_takes_forgeable_wire_carrier(sig)
                && minter_visibility_reaches_beyond_sink(vis)
            {
                self.out.push(format!(
                    "{}: minter `{name}` returns an admitted token AND takes a forgeable wire \
                     carrier ({FORGEABLE_WIRE_CARRIERS:?}) with visibility BROADER than its sink \
                     leaf (`vue_exec` / `svelte_exec`) — a sibling could forge the carrier and \
                     mint the token. Make the minter `pub(in …vue_exec)` / module-private, or back \
                     the token with a private resolved carrier.",
                    self.rel
                ));
            }
        }
    }
    impl<'ast> syn::visit::Visit<'ast> for V {
        fn visit_item_fn(&mut self, f: &'ast syn::ItemFn) {
            self.check(&f.sig, &f.attrs, &f.vis);
            syn::visit::visit_item_fn(self, f);
        }
        fn visit_impl_item_fn(&mut self, f: &'ast syn::ImplItemFn) {
            self.check(&f.sig, &f.attrs, &f.vis);
            syn::visit::visit_impl_item_fn(self, f);
        }
        fn visit_item_mod(&mut self, m: &'ast syn::ItemMod) {
            if mod_is_cfg_test(&m.attrs) {
                return; // test submodule — production-absent
            }
            syn::visit::visit_item_mod(self, m);
        }
    }
    let mut v = V {
        rel: rel.to_string(),
        out: Vec::new(),
    };
    syn::visit::Visit::visit_file(&mut v, file);
    v.out
}

#[test]
fn admitted_tokens_have_private_fields_and_seal() {
    let token_files = [
        "src/meta_resolve/projectors/publication_authority.rs",
        "src/typeinfo/framework_surface/vue_exec/mod.rs",
        "src/typeinfo/framework_surface/svelte_exec.rs",
    ];
    let mut all_facts = Vec::new();
    let mut minter_violations = Vec::new();
    for rel in token_files {
        let file = syn::parse_file(&read_rel(rel)).unwrap_or_else(|e| panic!("parse {rel}: {e}"));
        all_facts.extend(collect_token_struct_facts(&file));
        // (c) + (d): caller-supplied-kind + broad-visibility-minter checks.
        minter_violations.extend(admission_minter_violations(rel, &file));
    }
    // Anti-vacuity: every declared token struct was actually observed.
    let observed: std::collections::BTreeSet<&str> =
        all_facts.iter().map(|f| f.name.as_str()).collect();
    for tok in ADMITTED_TOKEN_STRUCTS {
        assert!(
            observed.contains(tok),
            "admitted token `{tok}` was NOT observed in the token files — it was renamed / moved \
             without updating the guard (anti-vacuity), or the parser regressed"
        );
    }
    assert!(
        minter_violations.is_empty(),
        "admitted-token minter-discipline violation(s) (caller-supplied kind / broad-visibility \
         forgeable-carrier minter):\n{}",
        minter_violations.join("\n")
    );
    let violations = token_struct_violations(&all_facts);
    assert!(
        violations.is_empty(),
        "admitted-token construction-discipline violation(s):\n{}",
        violations.join("\n")
    );
}

#[test]
fn admitted_tokens_construction_discipline_self_test_discriminates() {
    // KNOWN-GOOD: a token with all-private fields + a private `_seal`.
    let good = r#"
        pub(crate) struct AdmittedPublishedMember<'a> {
            owner: DeclIdentity,
            member: SurfaceMember,
            cursor: ProjectionCursor<'a>,
            _seal: Seal,
        }
    "#;
    let file = syn::parse_file(good).expect("parse good token");
    assert!(
        token_struct_violations(&collect_token_struct_facts(&file)).is_empty(),
        "self-test: an all-private-fields + private-seal token MUST pass"
    );

    // RED: a `pub` field (the struct-literal-from-parts escape).
    let pub_field = r#"
        pub(crate) struct AdmittedPublishedMember<'a> {
            pub member: SurfaceMember,
            cursor: ProjectionCursor<'a>,
            _seal: Seal,
        }
    "#;
    let file = syn::parse_file(pub_field).expect("parse pub-field token");
    let v = token_struct_violations(&collect_token_struct_facts(&file));
    assert!(
        v.iter()
            .any(|m| m.contains("AdmittedPublishedMember") && m.contains("NON-PRIVATE")),
        "self-test: a `pub member` field MUST FIRE (a public field lets a from_parts struct-literal \
         bypass admission); got: {v:?}"
    );

    // RED: a `pub(crate)` field is ALSO non-private (reachable for struct-literal
    // construction in-crate).
    let pub_crate_field = r#"
        pub(crate) struct ResolvedPayloadSurface {
            pub(crate) node: SemanticNodeId,
            kind: PublishedSurfaceKind,
            _seal: Seal,
        }
    "#;
    let file = syn::parse_file(pub_crate_field).expect("parse pub(crate)-field token");
    let v = token_struct_violations(&collect_token_struct_facts(&file));
    assert!(
        v.iter()
            .any(|m| m.contains("ResolvedPayloadSurface") && m.contains("NON-PRIVATE")),
        "self-test: a `pub(crate)` token field MUST FIRE; got: {v:?}"
    );

    // RED: a token MISSING its `_seal` (forgeable by a sibling that names all
    // fields).
    let no_seal = r#"
        pub(crate) struct SurfaceMemberCandidate {
            owner: DeclIdentity,
            member: SurfaceMember,
            kind: PublishedSurfaceKind,
        }
    "#;
    let file = syn::parse_file(no_seal).expect("parse no-seal token");
    let v = token_struct_violations(&collect_token_struct_facts(&file));
    assert!(
        v.iter()
            .any(|m| m.contains("SurfaceMemberCandidate") && m.contains("_seal")),
        "self-test: a token missing its private `_seal` MUST FIRE; got: {v:?}"
    );

    // ── check (c): caller-supplied `kind: PublishedSurfaceKind` BYPASS ──────
    // GREEN: the real production minters derive the kind internally — their
    // params are `macro_kind: AnalyzedMacroKind` / `payload: &ResolvedMacroPayload`,
    // never a by-value `kind: PublishedSurfaceKind`. A by-REFERENCE param is also
    // fine (it is not the caller-supplied-by-value bypass).
    let derived_kind_ok = r#"
        pub(crate) fn resolve_payload_surface(
            dispatch: &ProjectSemanticDispatch<'_>,
            payload: &ResolvedMacroPayload,
            expansion_kind: MacroExpansionKind,
            diag_sink: &mut Vec<MacroExpansionDiagnostics>,
        ) -> Option<ResolvedPayloadSurface> { None }
        fn published_surface_kind_for(macro_kind: AnalyzedMacroKind) -> PublishedSurfaceKind { todo!() }
        pub(crate) fn peek(kind: &PublishedSurfaceKind) -> bool { true }
    "#;
    let file = syn::parse_file(derived_kind_ok).expect("parse derived-kind-ok");
    assert!(
        admission_minter_violations("src/x.rs", &file).is_empty(),
        "self-test: a minter deriving kind internally (and a by-reference \
         `&PublishedSurfaceKind` reader) MUST pass check (c); got: {:?}",
        admission_minter_violations("src/x.rs", &file)
    );
    // RED: an admission fn with a BY-VALUE `kind: PublishedSurfaceKind` param —
    // the caller-supplied-kind bypass the ruling rejects.
    let caller_kind = r#"
        pub(crate) fn admit(
            candidate: SurfaceMemberCandidate,
            kind: PublishedSurfaceKind,
        ) -> Option<AdmittedPublishedMember> { None }
    "#;
    let file = syn::parse_file(caller_kind).expect("parse caller-kind");
    let v = admission_minter_violations("src/x.rs", &file);
    assert!(
        v.iter()
            .any(|m| m.contains("admit") && m.contains("BY-VALUE")),
        "self-test: an admission fn with a by-value `kind: PublishedSurfaceKind` param MUST FIRE \
         check (c); got: {v:?}"
    );
    // A `#[cfg(test)]`-gated ctor taking a by-value kind (the `admitted_for_test`
    // shape) is EXEMPT (production-only scan).
    let test_kind = r#"
        #[cfg(test)]
        pub(crate) fn admitted_for_test(
            kind: PublishedSurfaceKind,
        ) -> AdmittedPublishedMember { todo!() }
    "#;
    let file = syn::parse_file(test_kind).expect("parse test-kind");
    assert!(
        admission_minter_violations("src/x.rs", &file).is_empty(),
        "self-test: a #[cfg(test)] ctor with a by-value kind param MUST be exempt (production-only \
         scan)"
    );

    // ── check (d): broad-visibility minter taking a forgeable wire carrier ──
    // GREEN: the real Vue minter is `pub(in …vue_exec)` (stays within its sink
    // leaf), so even though it takes a `VueMacroSurface` and returns the token,
    // it does NOT fire.
    let vue_minter_ok = r#"
        pub(in crate::typeinfo::framework_surface::vue_exec) fn resolved_vue_surface(
            surface: VueMacroSurface,
        ) -> ResolvedVueSurface { todo!() }
    "#;
    let file = syn::parse_file(vue_minter_ok).expect("parse vue-minter-ok");
    assert!(
        admission_minter_violations("src/x.rs", &file).is_empty(),
        "self-test: a `pub(in …vue_exec)` minter taking a `VueMacroSurface` MUST pass check (d) \
         (it stays within its sink leaf); got: {:?}",
        admission_minter_violations("src/x.rs", &file)
    );
    // GREEN: a module-private Svelte minter taking a forgeable surface.
    let svelte_minter_ok = r#"
        fn macro_surface_shell(
            surface: TypeInfoSurface,
            macro_kind: AnalyzedMacroKind,
            owner: &str,
        ) -> SvelteResolvedSurface { todo!() }
    "#;
    let file = syn::parse_file(svelte_minter_ok).expect("parse svelte-minter-ok");
    assert!(
        admission_minter_violations("src/x.rs", &file).is_empty(),
        "self-test: a module-private Svelte minter taking a `TypeInfoSurface` MUST pass check (d); \
         got: {:?}",
        admission_minter_violations("src/x.rs", &file)
    );
    // RED: a `pub(crate)` minter taking a forgeable `VueMacroSurface` and
    // returning an admitted token — a sibling could forge the carrier + mint.
    let broad_minter = r#"
        pub(crate) fn forge_and_mint(
            surface: VueMacroSurface,
        ) -> ResolvedVueSurface { todo!() }
    "#;
    let file = syn::parse_file(broad_minter).expect("parse broad-minter");
    let v = admission_minter_violations("src/x.rs", &file);
    assert!(
        v.iter()
            .any(|m| m.contains("forge_and_mint") && m.contains("BROADER")),
        "self-test: a `pub(crate)` minter taking a forgeable wire carrier + returning a token MUST \
         FIRE check (d); got: {v:?}"
    );
    // RED: a fully `pub` minter taking a forgeable `TypeInfoSurface`.
    let pub_minter = r#"
        pub fn mint(
            surface: TypeInfoSurface,
        ) -> SvelteResolvedSurface { todo!() }
    "#;
    let file = syn::parse_file(pub_minter).expect("parse pub-minter");
    let v = admission_minter_violations("src/x.rs", &file);
    assert!(
        v.iter()
            .any(|m| m.contains("mint") && m.contains("BROADER")),
        "self-test: a `pub` minter taking a forgeable `TypeInfoSurface` MUST FIRE check (d); \
         got: {v:?}"
    );

    // The Svelte token's field-privacy facts are collected like any other (it is
    // in ADMITTED_TOKEN_STRUCTS) — a sealed Svelte token passes, a pub-field one
    // fires.
    let svelte_token_ok = r#"
        struct SvelteResolvedSurface {
            surface: VueMacroSurface,
            _seal: SvelteSurfaceSeal,
        }
    "#;
    let file = syn::parse_file(svelte_token_ok).expect("parse svelte-token-ok");
    assert!(
        token_struct_violations(&collect_token_struct_facts(&file)).is_empty(),
        "self-test: the sealed Svelte token (private fields + private `_seal`) MUST pass"
    );
    let svelte_token_pub = r#"
        struct SvelteResolvedSurface {
            pub surface: VueMacroSurface,
            _seal: SvelteSurfaceSeal,
        }
    "#;
    let file = syn::parse_file(svelte_token_pub).expect("parse svelte-token-pub");
    let v = token_struct_violations(&collect_token_struct_facts(&file));
    assert!(
        v.iter()
            .any(|m| m.contains("SvelteResolvedSurface") && m.contains("NON-PRIVATE")),
        "self-test: a `pub surface` field on the Svelte token MUST FIRE; got: {v:?}"
    );
}

// scanner_invariant: resolved_surface_access_impls_are_exactly_the_two_tokens
// scanner_justification: the COMPILER primary is the file-private `Sealed` supertrait in `resolved_surface_access.rs` (a sibling `impl Sealed` is `E0603`, so the trait is structurally implemented only there for the two tokens); this scanner is the residual inventory the compiler cannot express AT REVIEW TIME — it catches a future relaxation of the seal back to a named subtree-visible module (which would re-open the subtree-wide-seal forgeability hole, letting a `framework_surface` sibling impl the trait for a forged surface wrapper) by flagging any NEW impl self-type / blanket impl in the owner file.
// mechanism_ruling: structural-confinement-first — sealed file-private supertrait is the compiler primary; this exact-self-type-set inventory is the defense-in-depth residual.
// hardening_rounds: 0
// hardening_history: adoption — added in the publication-authority follow-up after the framework-surface seal was made genuinely module-private (both impls relocated into the owner file).
// ===========================================================================
// (D1-sealed-trait) ResolvedSurfaceAccess impl inventory.
//
// The compiler-PRIMARY for the framework-surface normalizer-input contract is
// the sealed `ResolvedSurfaceAccess` trait: its supertrait seal `Sealed` is
// PRIVATE to `resolved_surface_access.rs` (a bare module-private `trait`), and
// BOTH `impl ResolvedSurfaceAccess` (and `impl Sealed`) live in that one file.
// So a `framework_surface` sibling that tries `impl Sealed for ItsOwnWrapper`
// (to then `impl ResolvedSurfaceAccess` and drive the `pub(crate)` normalizers
// over a forged `VueMacroSurface`) is a COMPILE ERROR (`E0603` — `Sealed` is
// private), and the trait is STRUCTURALLY implemented only for the two tokens.
//
// This guard is the DEFENSE-IN-DEPTH residual inventory the compiler primary
// cannot itself express AT REVIEW TIME: it pins the `impl ResolvedSurfaceAccess`
// self-type set in the owner file to EXACTLY {ResolvedVueSurface,
// SvelteResolvedSurface} and FAILS on (a) a third concrete impl, (b) a blanket
// `impl<T> ResolvedSurfaceAccess for T` (a blanket would seal an OPEN set even
// inside the owner file). A future edit could relax the seal back to a named
// `pub(in …framework_surface)` module (re-opening the subtree-wide-seal
// forgeability hole — a sibling could then impl the trait for a forged surface
// wrapper) WITHOUT touching the two sanctioned impls; this inventory catches the
// symptom (a new self-type) even if the compiler-primary regresses.
// ===========================================================================

/// The EXACT sanctioned `impl ResolvedSurfaceAccess` self-type set (last-ident)
/// — the two framework-surface resolved-surface tokens. A self-type NOT in this
/// set, a MISSING sanctioned self-type, or ANY blanket impl fails the guard.
const RESOLVED_SURFACE_ACCESS_IMPL_SELF_TYPES: &[&str] =
    &["ResolvedVueSurface", "SvelteResolvedSurface"];

/// The result of inventorying the `impl ResolvedSurfaceAccess` impls in the
/// owner file `resolved_surface_access.rs`. The concrete self-type set must be
/// EXACTLY the two sanctioned tokens; a blanket impl seals an open set.
#[derive(Debug, Default, PartialEq, Eq)]
struct ResolvedSurfaceAccessImplInventory {
    /// The concrete self-type last-idents of every (non-test)
    /// `impl ResolvedSurfaceAccess for <Token>`.
    concrete_self_types: Vec<String>,
    /// Messages for any blanket / generic `impl<…> ResolvedSurfaceAccess for <T>`
    /// whose self-type is one of the impl's own generic type params.
    blanket_violations: Vec<String>,
}

/// Whether a trait path's LAST segment is `ResolvedSurfaceAccess`.
fn trait_is_resolved_surface_access(path: &syn::Path) -> bool {
    path.segments
        .last()
        .is_some_and(|s| s.ident == "ResolvedSurfaceAccess")
}

/// Recursively inventory every (non-test) `impl ResolvedSurfaceAccess for <T>`
/// in the parsed owner file. Records the concrete self-type (FULL `::`-path is
/// not needed here — these tokens are crate-local single idents) and flags any
/// blanket `impl<T> ResolvedSurfaceAccess for T`.
fn registered_resolved_surface_access_impls(
    file: &syn::File,
) -> ResolvedSurfaceAccessImplInventory {
    fn impl_is_test_gated(attrs: &[syn::Attribute]) -> bool {
        attrs.iter().any(|a| {
            a.path().is_ident("cfg")
                && matches!(&a.meta, syn::Meta::List(list)
                    if cfg_is_exactly_test_or_test_support(list.tokens.clone()))
        })
    }
    fn walk(items: &[syn::Item], inv: &mut ResolvedSurfaceAccessImplInventory) {
        for item in items {
            match item {
                syn::Item::Impl(imp) => {
                    let Some((_, trait_path, _)) = &imp.trait_ else {
                        continue;
                    };
                    if !trait_is_resolved_surface_access(trait_path) {
                        continue;
                    }
                    if impl_is_test_gated(&imp.attrs) {
                        continue;
                    }
                    let type_params: Vec<String> = imp
                        .generics
                        .params
                        .iter()
                        .filter_map(|p| match p {
                            syn::GenericParam::Type(tp) => Some(tp.ident.to_string()),
                            _ => None,
                        })
                        .collect();
                    let self_name = impl_self_ty_last_ident(&imp.self_ty)
                        .unwrap_or_else(|| "<impl>".to_string());
                    if type_params.contains(&self_name) {
                        inv.blanket_violations.push(format!(
                            "blanket/generic `impl<{}> ResolvedSurfaceAccess for {self_name}` — a \
                             blanket impl seals an OPEN set of types, letting any of them drive the \
                             `pub(crate)` framework-surface normalizers. Implement \
                             ResolvedSurfaceAccess EXACTLY for the two sanctioned resolved-surface \
                             tokens, never a type parameter",
                            type_params.join(", ")
                        ));
                    } else {
                        inv.concrete_self_types.push(self_name);
                    }
                }
                syn::Item::Mod(syn::ItemMod {
                    content: Some((_, inner)),
                    ..
                }) => walk(inner, inv),
                _ => {}
            }
        }
    }
    let mut inv = ResolvedSurfaceAccessImplInventory::default();
    walk(&file.items, &mut inv);
    inv
}

#[test]
fn resolved_surface_access_impls_are_exactly_the_two_tokens() {
    // The owner file is the SOLE place the trait can be implemented (the seal is
    // private to it). Pin the impl self-type set to EXACTLY the two tokens.
    let rel = "src/typeinfo/framework_surface/resolved_surface_access.rs";
    let file = syn::parse_file(&read_rel(rel)).unwrap_or_else(|e| panic!("parse {rel}: {e}"));
    let inv = registered_resolved_surface_access_impls(&file);

    assert!(
        inv.blanket_violations.is_empty(),
        "blanket `impl ResolvedSurfaceAccess` violation(s) in {rel} — a blanket impl seals an OPEN \
         self-type set:\n{}",
        inv.blanket_violations.join("\n")
    );

    let observed: std::collections::BTreeSet<&str> =
        inv.concrete_self_types.iter().map(|s| s.as_str()).collect();
    let sanctioned: std::collections::BTreeSet<&str> = RESOLVED_SURFACE_ACCESS_IMPL_SELF_TYPES
        .iter()
        .copied()
        .collect();

    // Anti-vacuity + exactness: the set must MATCH (a missing sanctioned impl, a
    // third impl, or a renamed token all fail).
    assert_eq!(
        observed, sanctioned,
        "the `impl ResolvedSurfaceAccess` self-type set in {rel} must be EXACTLY {sanctioned:?} \
         (the two framework-surface resolved-surface tokens); observed {observed:?}. A new \
         self-type is a NEW normalizer-input authority; a missing one means a token was renamed \
         without updating the guard. Both impls MUST stay in this owner file (the seal is private \
         to it)."
    );
}

#[test]
fn resolved_surface_access_impl_inventory_self_test_discriminates() {
    // GREEN: exactly the two sanctioned token impls (plus their `Sealed` impls,
    // which the inventory ignores). MUST pass the exactness check.
    let good = r#"
        trait Sealed {}
        pub(crate) trait ResolvedSurfaceAccess: Sealed { fn macro_surface(&self) -> &V; }
        impl Sealed for ResolvedVueSurface {}
        impl ResolvedSurfaceAccess for ResolvedVueSurface { fn macro_surface(&self) -> &V { todo!() } }
        impl Sealed for SvelteResolvedSurface {}
        impl ResolvedSurfaceAccess for SvelteResolvedSurface { fn macro_surface(&self) -> &V { todo!() } }
    "#;
    let file = syn::parse_file(good).expect("parse good");
    let inv = registered_resolved_surface_access_impls(&file);
    assert!(inv.blanket_violations.is_empty(), "good: no blanket");
    let observed: std::collections::BTreeSet<&str> =
        inv.concrete_self_types.iter().map(|s| s.as_str()).collect();
    let sanctioned: std::collections::BTreeSet<&str> = RESOLVED_SURFACE_ACCESS_IMPL_SELF_TYPES
        .iter()
        .copied()
        .collect();
    assert_eq!(
        observed, sanctioned,
        "self-test: exactly the two sanctioned token impls MUST satisfy the exactness check"
    );

    // RED: a THIRD concrete impl (a sibling slipped a forged wrapper in). The
    // observed set GAINS `ForgedSurface`, so it no longer equals the sanctioned
    // set.
    let third = r#"
        impl ResolvedSurfaceAccess for ResolvedVueSurface { fn macro_surface(&self) -> &V { todo!() } }
        impl ResolvedSurfaceAccess for SvelteResolvedSurface { fn macro_surface(&self) -> &V { todo!() } }
        impl ResolvedSurfaceAccess for ForgedSurface { fn macro_surface(&self) -> &V { todo!() } }
    "#;
    let file = syn::parse_file(third).expect("parse third");
    let inv = registered_resolved_surface_access_impls(&file);
    let observed: std::collections::BTreeSet<&str> =
        inv.concrete_self_types.iter().map(|s| s.as_str()).collect();
    let sanctioned: std::collections::BTreeSet<&str> = RESOLVED_SURFACE_ACCESS_IMPL_SELF_TYPES
        .iter()
        .copied()
        .collect();
    assert_ne!(
        observed, sanctioned,
        "self-test: a THIRD `impl ResolvedSurfaceAccess for ForgedSurface` MUST break the exactness \
         check (observed gains a non-sanctioned self-type)"
    );
    assert!(
        observed.contains("ForgedSurface"),
        "self-test: the third self-type MUST be inventoried; got {observed:?}"
    );

    // RED: a BLANKET impl seals an open set — must be flagged as a blanket
    // violation regardless of the named tokens.
    let blanket = r#"
        impl<T> ResolvedSurfaceAccess for T { fn macro_surface(&self) -> &V { todo!() } }
    "#;
    let file = syn::parse_file(blanket).expect("parse blanket");
    let inv = registered_resolved_surface_access_impls(&file);
    assert!(
        inv.blanket_violations
            .iter()
            .any(|m| m.contains("blanket") && m.contains("ResolvedSurfaceAccess")),
        "self-test: a blanket `impl<T> ResolvedSurfaceAccess for T` MUST FIRE the blanket check; \
         got: {:?}",
        inv.blanket_violations
    );

    // A `#[cfg(test)]`-gated impl is DROPPED (production-only scan) — a
    // `_for_test` helper impl must not count toward the inventory.
    let test_gated = r#"
        impl ResolvedSurfaceAccess for ResolvedVueSurface { fn macro_surface(&self) -> &V { todo!() } }
        impl ResolvedSurfaceAccess for SvelteResolvedSurface { fn macro_surface(&self) -> &V { todo!() } }
        #[cfg(test)]
        impl ResolvedSurfaceAccess for TestOnlySurface { fn macro_surface(&self) -> &V { todo!() } }
    "#;
    let file = syn::parse_file(test_gated).expect("parse test-gated");
    let inv = registered_resolved_surface_access_impls(&file);
    let observed: std::collections::BTreeSet<&str> =
        inv.concrete_self_types.iter().map(|s| s.as_str()).collect();
    assert!(
        !observed.contains("TestOnlySurface"),
        "self-test: a #[cfg(test)]-gated impl MUST be dropped (production-only scan); got {observed:?}"
    );
}

// ===========================================================================
// (D1-unsafe) Scoped no-`unsafe` guard.
//
// `unsafe` (transmute / union / `MaybeUninit` / raw-pointer writes) is the
// UNIVERSAL residual of every private-field / sealed fence in safe Rust: a
// `transmute` can fabricate any sealed token or unwrap any payload vault. The
// crate already contains `unsafe` elsewhere and is NOT `#![forbid(unsafe_code)]`,
// so this is a SCOPED ban over exactly the authority-callable sink scopes (the
// projectors incl. `output_sink` + `publication_authority`, the
// `typeinfo::framework_surface` sink, and the query-engine `surface` projector)
// — NOT a crate-wide forbid. Transmute stays an accepted safe-Rust residual
// elsewhere.
// ===========================================================================

/// `::`-joined module-path prefixes of the authority-callable scopes the
/// no-`unsafe` ban covers.
const NO_UNSAFE_SCOPE_PREFIXES: &[&str] = &[
    "crate::meta_resolve::projectors",
    "crate::typeinfo::framework_surface",
    "crate::resolver_core::component_meta_query_engine::surface",
    // The query-engine `registry_decl` now hosts the member-surface node-core +
    // the demand APIs (`materialize_pick_member_surface` /
    // `project_expr_surface_shape`); a `transmute` here could fabricate a
    // forgeable node past the demand-API seam, so the scoped ban covers it too.
    "crate::resolver_core::component_meta_query_engine::registry_decl",
];

/// Scan a parsed file for banned `unsafe` / `union` / `transmute` /
/// `MaybeUninit` surfaces, returning a violation string per hit.
fn unsafe_surface_violations(module_path: &str, file: &syn::File) -> Vec<String> {
    struct V {
        module_path: String,
        out: Vec<String>,
    }
    impl V {
        fn flag(&mut self, what: &str) {
            self.out
                .push(format!("`{}`: banned {what} in authority-callable scope — `unsafe` / `union` / `transmute` / `MaybeUninit` is forbidden here (it fabricates sealed tokens / unwraps the payload vault). This is a SCOPED ban, not crate-wide.", self.module_path));
        }
    }
    impl<'ast> syn::visit::Visit<'ast> for V {
        fn visit_item_mod(&mut self, m: &'ast syn::ItemMod) {
            if mod_is_cfg_test(&m.attrs) {
                return;
            }
            syn::visit::visit_item_mod(self, m);
        }
        fn visit_expr_unsafe(&mut self, e: &'ast syn::ExprUnsafe) {
            self.flag("`unsafe` block");
            syn::visit::visit_expr_unsafe(self, e);
        }
        fn visit_item_fn(&mut self, f: &'ast syn::ItemFn) {
            if fn_attrs_are_cfg_test(&f.attrs) {
                return;
            }
            if f.sig.unsafety.is_some() {
                self.flag("`unsafe fn`");
            }
            syn::visit::visit_item_fn(self, f);
        }
        fn visit_impl_item_fn(&mut self, f: &'ast syn::ImplItemFn) {
            if f.sig.unsafety.is_some() {
                self.flag("`unsafe fn`");
            }
            syn::visit::visit_impl_item_fn(self, f);
        }
        fn visit_item_union(&mut self, u: &'ast syn::ItemUnion) {
            self.flag("`union`");
            syn::visit::visit_item_union(self, u);
        }
        fn visit_item_impl(&mut self, i: &'ast syn::ItemImpl) {
            if i.unsafety.is_some() {
                self.flag("`unsafe impl`");
            }
            syn::visit::visit_item_impl(self, i);
        }
        fn visit_path(&mut self, p: &'ast syn::Path) {
            if p.segments.iter().any(|s| s.ident == "transmute")
                || p.segments.iter().any(|s| s.ident == "MaybeUninit")
            {
                self.flag("`transmute`/`MaybeUninit` reference");
            }
            syn::visit::visit_path(self, p);
        }
    }
    let mut v = V {
        module_path: module_path.to_string(),
        out: Vec::new(),
    };
    syn::visit::Visit::visit_file(&mut v, file);
    v.out
}

#[test]
fn authority_scopes_contain_no_unsafe() {
    let mut violations = Vec::new();
    let mut scanned = 0usize;
    for (rel, src) in production_src_files() {
        let module_path = module_path_for_rel(&rel);
        if !NO_UNSAFE_SCOPE_PREFIXES
            .iter()
            .any(|p| module_path == *p || module_path.starts_with(&format!("{p}::")))
        {
            continue;
        }
        let Ok(file) = syn::parse_file(&src) else {
            continue;
        };
        scanned += 1;
        violations.extend(unsafe_surface_violations(&module_path, &file));
    }
    assert!(
        scanned > 0,
        "expected to scan authority-callable scope files for unsafe; scanned none — the scope \
         prefixes regressed"
    );
    assert!(
        violations.is_empty(),
        "scoped no-`unsafe` violation(s) in the authority-callable sink scopes:\n{}",
        violations.join("\n")
    );
}

#[test]
fn authority_scopes_no_unsafe_self_test_discriminates() {
    // KNOWN-GOOD: ordinary safe code in scope.
    let good = syn::parse_file("fn ok() -> i32 { 1 + 1 }").expect("parse good");
    assert!(
        unsafe_surface_violations("crate::meta_resolve::projectors::output_sink", &good).is_empty(),
        "self-test: ordinary safe code MUST pass"
    );

    // RED: an `unsafe` block.
    let unsafe_block =
        syn::parse_file("fn leak() { unsafe { let _ = 1; } }").expect("parse unsafe block");
    let v = unsafe_surface_violations(
        "crate::meta_resolve::projectors::output_sink",
        &unsafe_block,
    );
    assert!(
        v.iter()
            .any(|m| m.contains("unsafe") && m.contains("block")),
        "self-test: an `unsafe` block MUST FIRE; got: {v:?}"
    );

    // RED: a `transmute` reference.
    let transmute =
        syn::parse_file("fn leak() { let _x: u64 = unsafe { std::mem::transmute(0u64) }; }")
            .expect("parse transmute");
    let v = unsafe_surface_violations("crate::typeinfo::framework_surface::vue_exec", &transmute);
    assert!(
        v.iter()
            .any(|m| m.contains("unsafe") || m.contains("transmute")),
        "self-test: a `transmute` reference MUST FIRE; got: {v:?}"
    );

    // RED: a `union` item.
    let union_item = syn::parse_file("union U { a: u32, b: f32 }").expect("parse union");
    let v = unsafe_surface_violations(
        "crate::resolver_core::component_meta_query_engine::surface",
        &union_item,
    );
    assert!(
        v.iter().any(|m| m.contains("union")),
        "self-test: a `union` item MUST FIRE; got: {v:?}"
    );

    // RED: an `unsafe fn`.
    let unsafe_fn = syn::parse_file("unsafe fn danger() {}").expect("parse unsafe fn");
    let v = unsafe_surface_violations(
        "crate::meta_resolve::projectors::publication_authority",
        &unsafe_fn,
    );
    assert!(
        v.iter().any(|m| m.contains("unsafe fn")),
        "self-test: an `unsafe fn` MUST FIRE; got: {v:?}"
    );
}
