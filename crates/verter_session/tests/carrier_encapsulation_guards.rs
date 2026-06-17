//! Anti-tail carrier ENCAPSULATION tripwires.
//!
//! The anti-tail invariant — no production site hand-binds a structural
//! carrier's `type_args` field, bypassing the sole descent accessor
//! [`SemanticNodeData::carrier_type_args`] — is enforced BY CONSTRUCTION: the
//! three structural carriers (`TypeOf` / `BareRef` / `ImportType`) are opaque
//! tuple payloads (`semantic_query::carrier`) with PRIVATE fields, and the
//! sanctioned accessor surface lives INSIDE `carrier.rs` (an
//! `impl SemanticNodeData` block alongside the private payloads). So the
//! raw-args surface is COMPILER-CONFINED to that one file: a sibling
//! `impl carrier::BareRefCarrier` in `semantic_query.rs` reading
//! `self.type_args` fails to compile (`E0616`, field private) and calling the
//! private `self.arg_nodes()` fails (`E0624`). rustc enforces that boundary on
//! the real compiled program — `cfg` / `#[path]` / `include!` / macro / alias
//! included — which a source scanner could never fully model.
//!
//! These small, LOCAL tripwires do NOT re-derive that compiler guarantee; they
//! only FREEZE the shape of the one trusted module so a future IN-FILE edit
//! cannot grow a second args surface without ALSO editing the guard. Tripwire 2
//! is therefore a STRICT EXACT-SHAPE ALLOWLIST: it accepts `carrier.rs` ONLY if
//! every item matches the precise known-good shape and REJECTS literally
//! everything else — so there is no remaining syntactic evasion unless the
//! future edit also changes or removes the guard.
//!
//! The four tripwires:
//!   1. [`carrier_variants_are_opaque_tuple_payloads`] — each carrier variant on
//!      `SemanticNodeData` wraps its OPAQUE payload by its FULL path
//!      `carrier::{Name}Carrier` (an unqualified / wrong-module / raw
//!      `Arc<[SemanticNodeId]>` payload is rejected).
//!   2. [`carrier_module_has_no_public_type_args_surface`] — the exact-shape
//!      allowlist. `carrier.rs` is accepted ONLY if it contains EXACTLY: the two
//!      sanctioned `use` imports (no renames, no extras); the three head-view
//!      aliases by their exact definition; the three carrier structs with their
//!      EXACT private field sets and the five built-in derives only; one private
//!      inherent impl per carrier with its EXACT private method signatures; and
//!      one `impl SemanticNodeData` with EXACTLY the eight sanctioned accessors
//!      at their exact visibility + signatures. No body may contain a macro
//!      invocation, and no body outside the sanctioned descent/rebuild set may
//!      read a carrier's raw `type_args` field / `arg_nodes()`. The
//!      `mod carrier;` declaration in `semantic_query.rs` must be unadorned (no
//!      `#[path]` / `#[cfg]` / inline body). Any other item / shape / attribute
//!      is a violation.
//!   3. [`carrier_type_args_accessor_is_exhaustive_and_wildcard_free`] and
//!   4. [`map_carrier_type_args_is_exhaustive_and_wildcard_free`] — the descent
//!      accessor and the rebuild channel keep an EXHAUSTIVE, catch-all-free
//!      `match self` (a new variant fails to COMPILE there), inspecting every
//!      `match self` specifically and treating `_` / `(_)` / `&_` / `x @ _` /
//!      `A | _` all as catch-alls.
//!
//! The `*_discriminates` self-tests below feed SYNTHETIC inputs and prove each
//! check ACCEPTS the real sealed `carrier.rs` and REJECTS each deviation
//! (red→green: reverting a check to its weak form makes the matching assertion
//! fail).

use std::collections::BTreeSet;
use std::path::PathBuf;

use quote::ToTokens;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("repo root")
        .to_path_buf()
}

fn read_workspace_file(rel: &str) -> String {
    std::fs::read_to_string(workspace_root().join(rel))
        .unwrap_or_else(|e| panic!("read {rel}: {e}"))
}

const SEMANTIC_QUERY_RS: &str = "crates/verter_session/src/semantic_query.rs";
const CARRIER_RS: &str = "crates/verter_session/src/semantic_query/carrier.rs";

/// The three structural carrier variants the anti-tail rule polices
/// (`Foo<Arg>` / `typeof f<Arg>` / `import("m").G<Arg>`).
const CARRIER_VARIANTS: [&str; 3] = ["TypeOf", "BareRef", "ImportType"];
const CARRIER_STRUCTS: [&str; 3] = ["TypeOfCarrier", "BareRefCarrier", "ImportTypeCarrier"];

/// The only derives the carrier structs may carry — the exact built-ins. A
/// custom or qualified derive could synthesise a leaking trait impl a syn
/// `#[derive(...)]` scan would treat as benign, so the set is CLOSED (each
/// carrier must derive EXACTLY these five, by bare single-segment ident).
const ALLOWED_DERIVES: [&str; 5] = ["Clone", "PartialEq", "Eq", "Hash", "Debug"];

// ════════════════════════════════════════════════════════════════════
// EXACT-SHAPE description tables. The exact-shape allowlist accepts an item
// ONLY if it matches the corresponding entry here; anything not described is
// rejected, and a described item that is missing is a violation. Pinning the
// shape declaratively makes the checks read as "the module IS this and nothing
// else".
// ════════════════════════════════════════════════════════════════════

/// The EXACT `use` imports carrier.rs may carry — no renames, no extras, no
/// fewer. Compared as normalized token strings, so a `use … as …` rename, an
/// added import, or a removed import is a deviation.
const EXPECTED_USES: [&str; 2] = [
    "use std::sync::Arc;",
    "use super::{NodeScopeId, SemanticNodeData, SemanticNodeId, ValueRootKey};",
];

/// The EXACT head-view aliases (full definitions, attrs ignored). Pinned by
/// normalized token string, so a different RHS — e.g. one resolving to
/// `&[SemanticNodeId]` args, or aliasing a carrier struct — is a deviation. The
/// three names are also the ONLY `type` aliases carrier.rs may define.
const EXPECTED_HEAD_ALIASES: [(&str, &str); 3] = [
    (
        "TypeOfHead",
        "pub(crate) type TypeOfHead<'a> = (&'a ValueRootKey, &'a Arc<[Arc<str>]>);",
    ),
    (
        "BareRefHead",
        "pub(crate) type BareRefHead<'a> = (&'a Arc<str>, &'a NodeScopeId);",
    ),
    (
        "ImportTypeHead",
        "pub(crate) type ImportTypeHead<'a> = (&'a Arc<str>, &'a Arc<[Arc<str>]>, bool);",
    ),
];

/// The exact shape of one carrier struct + its private inherent impl: the
/// ordered private field set (name, type) and the private inherent methods
/// (name, full signature). Both are EXACT — an extra/missing/renamed/retyped
/// field or method is a deviation.
struct CarrierSpec {
    name: &'static str,
    fields: &'static [(&'static str, &'static str)],
    methods: &'static [(&'static str, &'static str)],
}

const CARRIER_SPECS: [CarrierSpec; 3] = [
    CarrierSpec {
        name: "TypeOfCarrier",
        fields: &[
            ("value_root", "ValueRootKey"),
            ("path", "Arc<[Arc<str>]>"),
            ("type_args", "Arc<[SemanticNodeId]>"),
        ],
        methods: &[
            (
                "new",
                "fn new(value_root: ValueRootKey, path: Arc<[Arc<str>]>, type_args: Arc<[SemanticNodeId]>) -> Self",
            ),
            ("value_root", "fn value_root(&self) -> &ValueRootKey"),
            ("path", "fn path(&self) -> &Arc<[Arc<str>]>"),
            ("arg_nodes", "fn arg_nodes(&self) -> &[SemanticNodeId]"),
            (
                "with_type_args",
                "fn with_type_args(&self, type_args: Arc<[SemanticNodeId]>) -> Self",
            ),
        ],
    },
    CarrierSpec {
        name: "BareRefCarrier",
        fields: &[
            ("name", "Arc<str>"),
            ("scope", "NodeScopeId"),
            ("type_args", "Arc<[SemanticNodeId]>"),
        ],
        methods: &[
            (
                "new",
                "fn new(name: Arc<str>, scope: NodeScopeId, type_args: Arc<[SemanticNodeId]>) -> Self",
            ),
            ("name", "fn name(&self) -> &Arc<str>"),
            ("scope", "fn scope(&self) -> &NodeScopeId"),
            ("arg_nodes", "fn arg_nodes(&self) -> &[SemanticNodeId]"),
            (
                "with_type_args",
                "fn with_type_args(&self, type_args: Arc<[SemanticNodeId]>) -> Self",
            ),
        ],
    },
    CarrierSpec {
        name: "ImportTypeCarrier",
        fields: &[
            ("specifier", "Arc<str>"),
            ("qualifier", "Arc<[Arc<str>]>"),
            ("type_args", "Arc<[SemanticNodeId]>"),
            ("typeof_query", "bool"),
        ],
        methods: &[
            (
                "new",
                "fn new(specifier: Arc<str>, qualifier: Arc<[Arc<str>]>, type_args: Arc<[SemanticNodeId]>, typeof_query: bool) -> Self",
            ),
            ("specifier", "fn specifier(&self) -> &Arc<str>"),
            ("qualifier", "fn qualifier(&self) -> &Arc<[Arc<str>]>"),
            ("typeof_query", "fn typeof_query(&self) -> bool"),
            ("arg_nodes", "fn arg_nodes(&self) -> &[SemanticNodeId]"),
            (
                "with_type_args",
                "fn with_type_args(&self, type_args: Arc<[SemanticNodeId]>) -> Self",
            ),
        ],
    },
];

/// The exact shape of one sanctioned accessor on `impl SemanticNodeData`:
/// its name, expected visibility, and full signature. `impl SemanticNodeData`
/// must contain EXACTLY these eight and NO other method.
struct AccessorSpec {
    name: &'static str,
    vis: &'static str,
    sig: &'static str,
}

const ACCESSOR_SPECS: [AccessorSpec; 8] = [
    AccessorSpec {
        name: "carrier_type_args",
        vis: "pub(crate)",
        sig: "fn carrier_type_args(&self) -> &[SemanticNodeId]",
    },
    AccessorSpec {
        name: "map_carrier_type_args",
        vis: "pub(crate)",
        sig: "fn map_carrier_type_args(&self, new_args: Arc<[SemanticNodeId]>) -> Option<Self>",
    },
    AccessorSpec {
        name: "new_typeof",
        vis: "pub",
        sig: "fn new_typeof(value_root: ValueRootKey, path: Arc<[Arc<str>]>, type_args: Arc<[SemanticNodeId]>) -> Self",
    },
    AccessorSpec {
        name: "new_bare_ref",
        vis: "pub",
        sig: "fn new_bare_ref(name: Arc<str>, scope: NodeScopeId, type_args: Arc<[SemanticNodeId]>) -> Self",
    },
    AccessorSpec {
        name: "new_import_type",
        vis: "pub",
        sig: "fn new_import_type(specifier: Arc<str>, qualifier: Arc<[Arc<str>]>, type_args: Arc<[SemanticNodeId]>, typeof_query: bool) -> Self",
    },
    AccessorSpec {
        name: "typeof_head",
        vis: "pub(crate)",
        sig: "fn typeof_head(&self) -> Option<TypeOfHead<'_>>",
    },
    AccessorSpec {
        name: "bare_ref_head",
        vis: "pub(crate)",
        sig: "fn bare_ref_head(&self) -> Option<BareRefHead<'_>>",
    },
    AccessorSpec {
        name: "import_type_head",
        vis: "pub(crate)",
        sig: "fn import_type_head(&self) -> Option<ImportTypeHead<'_>>",
    },
];

/// The methods sanctioned to contain a raw `.type_args` field read or an
/// `.arg_nodes()` method call: the DESCENT (`carrier_type_args` calls the
/// private `arg_nodes`; `arg_nodes` reads `self.type_args`) and the REBUILD
/// (`map_carrier_type_args` delegates to the private `with_type_args`). Every
/// other body in carrier.rs must NOT touch a carrier's raw args.
const SANCTIONED_RAW_BODIES: [&str; 4] = [
    "carrier_type_args",
    "arg_nodes",
    "with_type_args",
    "map_carrier_type_args",
];

fn parse(rel: &str) -> syn::File {
    let src = read_workspace_file(rel);
    syn::parse_file(&src).unwrap_or_else(|e| panic!("{rel} must parse as Rust: {e}"))
}

/// TRIPWIRE 1 (DISCRIMINATING). The three carrier variants on
/// `SemanticNodeData` must be OPAQUE single-field tuple payloads
/// (`TypeOf(carrier::TypeOfCarrier)`), never named-struct variants
/// (`TypeOf { value_root, path, type_args }`). A named-struct variant
/// re-exposes a directly bindable `type_args` field at every match site —
/// the precise anti-tail shape this block makes unrepresentable.
///
/// Pins the payload to its FULL PATH
/// `carrier::{Name}Carrier` via [`carrier_variant_payload_is_opaque`] — NOT
/// merely the final type-path segment. A raw-tuple payload
/// `BareRef(Arc<[SemanticNodeId]>)` is ALSO a single-field
/// `syn::Fields::Unnamed` — so the arity check alone would accept it — and an
/// unqualified `BareRef(BareRefCarrier)` (the shape a raw
/// `type BareRefCarrier = Arc<[SemanticNodeId]>` alias takes) or a wrong-module
/// `BareRef(other::BareRefCarrier)` would pass a final-segment-only check; each
/// reopens positional `SemanticNodeData::BareRef(type_args)` binding at every
/// match site. Requiring the EXACT two-segment `carrier :: {Name}Carrier` path
/// rejects all of them (a `type` alias resolves to a different path a syn scan
/// cannot follow — which is precisely why exact-path is the right defense). The
/// discrimination is proven by
/// [`carrier_variant_payload_type_check_discriminates`].
///
/// Fails against the pre-change tree: there the variants are
/// `syn::Fields::Named` carrying a `type_args` field.
#[test]
fn carrier_variants_are_opaque_tuple_payloads() {
    let file = parse(SEMANTIC_QUERY_RS);
    let mut found: BTreeSet<String> = BTreeSet::new();

    for item in &file.items {
        let syn::Item::Enum(en) = item else { continue };
        if en.ident != "SemanticNodeData" {
            continue;
        }
        for v in &en.variants {
            let name = v.ident.to_string();
            if !CARRIER_VARIANTS.contains(&name.as_str()) {
                continue;
            }
            found.insert(name.clone());
            match &v.fields {
                syn::Fields::Unnamed(f) => {
                    assert_eq!(
                        f.unnamed.len(),
                        1,
                        "ANTI-TAIL ENCAPSULATION: carrier variant `{name}` must wrap exactly one \
                         opaque payload (`{name}(carrier::{name}Carrier)`); found {} tuple fields",
                        f.unnamed.len()
                    );
                    // The single payload's TYPE PATH must be
                    // EXACTLY the two-segment opaque path `carrier::{name}Carrier`. A
                    // final-segment-only check would accept an unqualified
                    // `{name}({name}Carrier)` (the shape a raw
                    // `type {name}Carrier = Arc<[SemanticNodeId]>` alias takes), a
                    // wrong-module `{name}(other::{name}Carrier)`, or a raw-tuple
                    // `{name}(Arc<[SemanticNodeId]>)` — each of which re-opens positional
                    // `type_args` binding at every match site.
                    let expected = format!("{name}Carrier");
                    assert!(
                        carrier_variant_payload_is_opaque(v, &expected),
                        "ANTI-TAIL ENCAPSULATION: carrier variant `{name}` must wrap the OPAQUE \
                         payload by its FULL path `carrier::{expected}` (private fields). Found \
                         payload type path `{found:?}` — an unqualified `{name}({expected})`, a \
                         wrong-module `{name}(other::{expected})`, or a raw/alias \
                         `{name}(Arc<[SemanticNodeId]>)` re-opens positional `type_args` binding \
                         and is FORBIDDEN.",
                        found = carrier_variant_payload_type_path(v),
                    );
                }
                syn::Fields::Named(_) => panic!(
                    "ANTI-TAIL ENCAPSULATION: carrier variant `{name}` is a NAMED-STRUCT variant, \
                     which re-exposes a bindable `type_args` field at every match site. It MUST be \
                     an opaque tuple payload `{name}(carrier::{name}Carrier)` whose fields are \
                     private — making the `node.type_args` anti-tail bind unrepresentable."
                ),
                syn::Fields::Unit => panic!(
                    "carrier variant `{name}` must carry a `carrier::{name}Carrier` payload, not be a \
                     unit variant"
                ),
            }
        }
    }

    assert_eq!(
        found,
        CARRIER_VARIANTS
            .iter()
            .map(|s| s.to_string())
            .collect::<BTreeSet<_>>(),
        "all three carrier variants must exist on `SemanticNodeData`; found {found:?}"
    );
}

/// TRIPWIRE 2 (DISCRIMINATING). carrier.rs is the SOLE module that can reach a
/// carrier's raw args (the payload methods are private; a sibling
/// `impl carrier::BareRefCarrier` reading `self.type_args` fails `E0616` and
/// calling `self.arg_nodes()` fails `E0624`). So this file-scoped guard is
/// COMPLETE only if it polices the WHOLE module shape — and it does so as a
/// STRICT EXACT-SHAPE ALLOWLIST: [`carrier_module_shape_violations`] accepts
/// carrier.rs ONLY if every item matches its precise known-good shape and
/// rejects everything else, while [`carrier_module_decl_violations`] pins the
/// unadorned `mod carrier;` declaration. There is no remaining syntactic
/// evasion unless the future edit also changes the guard.
///
/// Discrimination is proven by [`carrier_exact_shape_allowlist_discriminates`]
/// (the per-rule rejections) and [`carrier_module_decl_discriminates`] (the
/// module-decl rule).
///
/// Fails against any tree where carrier.rs is absent or where ANY item deviates
/// from the exact shape (an extra/renamed import, an off-spec head alias, a
/// helper struct, a changed field/method signature, a non-private carrier
/// member, an extra or missing accessor, a macro in a body, a raw-args read
/// outside the sanctioned bodies, an adorned `mod carrier` decl, …).
#[test]
fn carrier_module_has_no_public_type_args_surface() {
    let path = workspace_root().join(CARRIER_RS);
    assert!(
        path.is_file(),
        "ANTI-TAIL ENCAPSULATION: the opaque carrier module `{CARRIER_RS}` must exist — its \
         carriers' PRIVATE fields are what make the anti-tail `node.type_args` bind unrepresentable."
    );

    let mut violations = carrier_module_decl_violations(&parse(SEMANTIC_QUERY_RS));
    violations.extend(carrier_module_shape_violations(&parse(CARRIER_RS)));

    assert!(
        violations.is_empty(),
        "ANTI-TAIL ENCAPSULATION: carrier.rs must match the EXACT sealed shape (the strict \
         allowlist), but found {} deviation(s):\n  - {}",
        violations.len(),
        violations.join("\n  - ")
    );
}

/// TRIPWIRE 3 (RETAINED-INVARIANT regression guard). The sole descent
/// accessor `SemanticNodeData::carrier_type_args` (DEFINED in carrier.rs, the
/// sealed accessor module) must stay an
/// EXHAUSTIVE, catch-all-free match: NO arm may be an irrefutable catch-all — not a
/// top-level `_ =>` / `_ if`, not a bare-binding `other =>` (a `Pat::Ident`),
/// not a parenthesised `(_) =>` / reference `&_ =>` / aliasing `x @ _ =>`, and
/// not a `|`-pattern hiding any of those ([`arm_is_catchall`]). A catch-all
/// would silently return `&[]` for a future carrier that grows args; instead a
/// new variant must fail to compile here, forcing its author to classify it.
/// This is the compile-fence that survives the retirement of the source
/// scanner.
///
/// A bare-binding `other => &[]` (a `Pat::Ident`, NOT a `Pat::Wild`) is a
/// catch-all too, so the detector RECURSIVELY UNWRAPS
/// `Pat::Paren` / `Pat::Reference` / `Pat::Or` / `Pat::Ident`-with-subpattern,
/// so `(_) =>`, `&_ =>`, `x @ _ =>`, and `A | _ =>` all fail as catch-alls.
/// Match-self targeting: the check inspects EVERY
/// `match self { … }` in the body ([`match_self_exprs`]), not merely the FIRST
/// `match` in the block. A future body with a harmless leading
/// `match something_else { … }` followed by the real `match self { _ => &[] }`
/// would have let the old first-match locator inspect the harmless match and
/// pass, never seeing the catch-all in `match self`. Discrimination is proven by
/// [`accessor_catchall_detector_discriminates`] (catch-all detection) and
/// [`accessor_match_self_targeting_discriminates`] (match-self targeting).
#[test]
fn carrier_type_args_accessor_is_exhaustive_and_wildcard_free() {
    // `carrier_type_args` is DEFINED in carrier.rs (the sealed accessor
    // module), so parse carrier.rs.
    let file = parse(CARRIER_RS);

    let mut checked = false;
    for item in &file.items {
        let syn::Item::Impl(im) = item else { continue };
        // `impl SemanticNodeData { … }` (inherent impl, no trait).
        if im.trait_.is_some() {
            continue;
        }
        if !type_path_is(&im.self_ty, "SemanticNodeData") {
            continue;
        }
        for ii in &im.items {
            let syn::ImplItem::Fn(f) = ii else { continue };
            if f.sig.ident != "carrier_type_args" {
                continue;
            }
            checked = true;
            // Inspect EVERY `match self`, not the first `match` in the block, so a
            // catch-all in `match self` cannot hide behind a leading
            // catch-all-free `match other`.
            let match_selves = match_self_exprs(&f.block);
            assert!(
                !match_selves.is_empty(),
                "carrier_type_args body must contain a `match self {{ … }}` (the carrier-arg \
                 dispatch); none found."
            );
            for match_expr in &match_selves {
                for arm in &match_expr.arms {
                    assert!(
                        !arm_is_catchall(&arm.pat),
                        "REGRESSION: `carrier_type_args`'s `match self` must stay catch-all-free — \
                         every arm must name a variant (`Self::Foo(..)` / \
                         `SemanticNodeData::Foo {{ .. }}`). A catch-all (`_ =>`, a bare-binding \
                         `other =>`, a `(_) =>` / `&_ =>` / `x @ _ =>`, or a `|`-pattern hiding \
                         any) would silently return `&[]` for a future carrier, dropping its args; \
                         the exhaustive variant enumeration is the new-variant compile-fence."
                    );
                }
            }
        }
    }
    assert!(
        checked,
        "did not find `fn carrier_type_args` in an inherent `impl SemanticNodeData` — the descent \
         accessor must exist and stay the sole structural carrier-arg channel."
    );
}

/// TRIPWIRE 4 (RETAINED-INVARIANT regression guard). The carrier REBUILD
/// channel `SemanticNodeData::map_carrier_type_args` must stay an EXHAUSTIVE,
/// catch-all-free match — the same compile-fence as the descent accessor
/// (tripwire 3). `map_carrier_type_args` is the SOLE crate-wide carrier
/// reconstruction channel (the carriers' own `with_type_args` rebuild is
/// PRIVATE to the carrier module); a `_ => None` catch-all would
/// silently refuse to rebuild a
/// future carrier that grows args, dropping a substitution. Forcing a new
/// variant to fail compilation here makes its author classify it.
///
/// Fails against a tree where `map_carrier_type_args` ends in a
/// `_ => None` wildcard arm. Match-self targeting: like tripwire 3, inspects
/// EVERY `match self` ([`match_self_exprs`]), not the first `match` in the block,
/// so a catch-all in `match self` cannot hide behind a leading `match other`.
/// Reuses [`arm_is_catchall`] (proven discriminating by
/// [`accessor_catchall_detector_discriminates`]; match-self targeting proven by
/// [`accessor_match_self_targeting_discriminates`]).
#[test]
fn map_carrier_type_args_is_exhaustive_and_wildcard_free() {
    // `map_carrier_type_args` is DEFINED in carrier.rs (the sealed accessor
    // module).
    let file = parse(CARRIER_RS);

    let mut checked = false;
    for item in &file.items {
        let syn::Item::Impl(im) = item else { continue };
        // `impl SemanticNodeData { … }` (inherent impl, no trait).
        if im.trait_.is_some() {
            continue;
        }
        if !type_path_is(&im.self_ty, "SemanticNodeData") {
            continue;
        }
        for ii in &im.items {
            let syn::ImplItem::Fn(f) = ii else { continue };
            if f.sig.ident != "map_carrier_type_args" {
                continue;
            }
            checked = true;
            let match_selves = match_self_exprs(&f.block);
            assert!(
                !match_selves.is_empty(),
                "map_carrier_type_args body must contain a `match self {{ … }}` (the carrier \
                 rebuild dispatch); none found."
            );
            for match_expr in &match_selves {
                for arm in &match_expr.arms {
                    assert!(
                        !arm_is_catchall(&arm.pat),
                        "REGRESSION: `map_carrier_type_args`'s `match self` must stay \
                         catch-all-free (no `_ => None`, no bare-binding catch-all). A wildcard \
                         would silently refuse to rebuild a future carrier that grew a \
                         `type_args` field, dropping a substitution; the exhaustive non-carrier \
                         enumeration returning `None` is the new-variant compile-fence (mirrors \
                         `carrier_type_args`)."
                    );
                }
            }
        }
    }
    assert!(
        checked,
        "did not find `fn map_carrier_type_args` in an inherent `impl SemanticNodeData` — the \
         carrier rebuild channel must exist and stay the sole reconstruction path."
    );
}

fn type_path_is(ty: &syn::Type, ident: &str) -> bool {
    matches!(ty, syn::Type::Path(p) if p.path.segments.last().map(|s| s.ident == ident).unwrap_or(false))
}

/// WEAK-FORM locator (retained only as the
/// [`accessor_match_self_targeting_discriminates`] cross-check). Returns the
/// FIRST `match` expression inside a function block — which is NOT necessarily
/// the `match self` the accessor dispatches on. A body with a leading
/// `match other { … }` returns that harmless match, so a catch-all in the real
/// `match self` tail would be missed. The production guards use
/// [`match_self_exprs`] instead; this helper exists to PROVE the old form's
/// blind spot.
fn find_first_match(block: &syn::Block) -> Option<&syn::ExprMatch> {
    fn from_expr(expr: &syn::Expr) -> Option<&syn::ExprMatch> {
        match expr {
            syn::Expr::Match(m) => Some(m),
            syn::Expr::Block(b) => from_block(&b.block),
            _ => None,
        }
    }
    fn from_block(block: &syn::Block) -> Option<&syn::ExprMatch> {
        for stmt in &block.stmts {
            if let syn::Stmt::Expr(e, _) = stmt {
                if let Some(m) = from_expr(e) {
                    return Some(m);
                }
            }
        }
        None
    }
    from_block(block)
}

/// Match-self targeting helper. Collect EVERY `match self { … }` expression in a function
/// body — recursively over the whole expression tree (via `syn::visit`), so a
/// `match self` is found whether it is the tail expression or follows leading
/// statements (a harmless `let x = match other { … };`, a `match other { … };`
/// statement, an `if`/block wrapper, …). The guards inspect every `match self`
/// it returns, so a catch-all in the real `match self` cannot hide behind a
/// leading catch-all-free `match other` (the blind spot of the first-match
/// locator [`find_first_match`]).
fn match_self_exprs(block: &syn::Block) -> Vec<&syn::ExprMatch> {
    let mut collector = MatchSelfCollector { found: Vec::new() };
    syn::visit::Visit::visit_block(&mut collector, block);
    collector.found
}

struct MatchSelfCollector<'ast> {
    found: Vec<&'ast syn::ExprMatch>,
}

impl<'ast> syn::visit::Visit<'ast> for MatchSelfCollector<'ast> {
    fn visit_expr_match(&mut self, node: &'ast syn::ExprMatch) {
        if expr_is_self(&node.expr) {
            self.found.push(node);
        }
        // Descend so a `match self` nested inside another expression is still
        // found (and a nested `match self` inside an arm body is caught too).
        syn::visit::visit_expr_match(self, node);
    }
}

/// True iff `expr` is exactly the `self` receiver — the scrutinee of the
/// accessor's `match self { … }`. Compared via the token stream so the keyword
/// ident `self` is matched unambiguously (`match other` / `match self.kind()`
/// are NOT `self`).
fn expr_is_self(expr: &syn::Expr) -> bool {
    expr.to_token_stream().to_string() == "self"
}

// ════════════════════════════════════════════════════════════════════
// Tripwire-1 payload-path helpers + the carrier-name / self-ty helpers reused
// by the exact-shape scan and the self-tests.
// ════════════════════════════════════════════════════════════════════

/// FULL type-path segments of `variant`'s
/// single unnamed (tuple) field, or `None` if the variant is not a single-field
/// tuple variant (named-struct / unit / multi-field) or its payload is not a
/// path type. `BareRef(carrier::BareRefCarrier)` → `Some(["carrier",
/// "BareRefCarrier"])`; `BareRef(BareRefCarrier)` → `Some(["BareRefCarrier"])`;
/// `BareRef(other::BareRefCarrier)` → `Some(["other", "BareRefCarrier"])`;
/// `BareRef(Arc<[SemanticNodeId]>)` → `Some(["Arc"])`.
fn carrier_variant_payload_type_path(variant: &syn::Variant) -> Option<Vec<String>> {
    let syn::Fields::Unnamed(f) = &variant.fields else {
        return None;
    };
    if f.unnamed.len() != 1 {
        return None;
    }
    type_path_segments(&f.unnamed.first()?.ty)
}

/// True iff `variant`'s payload type path is EXACTLY
/// the two-segment opaque carrier path `carrier::{expected_struct}` — rejecting
/// an unqualified `{expected_struct}` (one segment — the shape a raw `type`
/// alias for the arg slice would take), a wrong-module `other::{expected_struct}`,
/// a raw/alias `Arc<[SemanticNodeId]>`, and any named-struct / multi-field /
/// non-path shape. A `syn` scan cannot resolve a `type` alias, so requiring the
/// exact `carrier::` qualifier IS the defense: an alias has a different path
/// and is rejected.
fn carrier_variant_payload_is_opaque(variant: &syn::Variant, expected_struct: &str) -> bool {
    match carrier_variant_payload_type_path(variant) {
        Some(segs) => segs.len() == 2 && segs[0] == "carrier" && segs[1] == expected_struct,
        None => false,
    }
}

/// Final path-segment ident of a `Type::Path` (`carrier::BareRefCarrier` →
/// `"BareRefCarrier"`, `Arc<[SemanticNodeId]>` → `"Arc"`). `None` for a
/// non-path type (reference / tuple / slice / …).
fn type_path_last_ident(ty: &syn::Type) -> Option<String> {
    match ty {
        syn::Type::Path(p) => p.path.segments.last().map(|s| s.ident.to_string()),
        _ => None,
    }
}

/// All path-segment idents of a `Type::Path` in order (`carrier::BareRefCarrier`
/// → `["carrier", "BareRefCarrier"]`, `Arc<[SemanticNodeId]>` → `["Arc"]`).
/// `None` for a non-path type (reference / tuple / slice / …). The full path —
/// not just the final segment — is what lets [`carrier_variant_payload_is_opaque`]
/// reject an unqualified or wrong-module payload whose final segment happens to
/// match the expected carrier name.
fn type_path_segments(ty: &syn::Type) -> Option<Vec<String>> {
    match ty {
        syn::Type::Path(p) => Some(
            p.path
                .segments
                .iter()
                .map(|s| s.ident.to_string())
                .collect(),
        ),
        _ => None,
    }
}

/// True iff `vis` exposes an item BEYOND `carrier.rs` itself — anything other
/// than private (`Inherited`) or strictly module-local `pub(self)`. `pub(super)`
/// COUNTS as escaping: re-exposing a carrier method even to the ~6000-line
/// parent `semantic_query` module is the leak. The carrier PAYLOAD methods must
/// all be non-escaping; only the eight sanctioned `SemanticNodeData` accessors
/// are crate-visible (their exact visibility is pinned separately).
fn vis_escapes_module(vis: &syn::Visibility) -> bool {
    match vis {
        syn::Visibility::Inherited => false,
        syn::Visibility::Public(_) => true,
        syn::Visibility::Restricted(r) => !r.path.is_ident("self"),
    }
}

/// Peel `&T` / `(T)` / `{ T }` wrappers (`syn::Type::Reference` / `Type::Paren`
/// / `Type::Group`) down to the inner type. A manual trait impl on
/// `&BareRefCarrier` (or `(BareRefCarrier)`) targets the carrier just as
/// `BareRefCarrier` does, so the carrier-name checks unwrap the self-ty first;
/// a direct `type_path_last_ident(self_ty)` returns `None` for a
/// reference self-ty and would MISS it.
fn unwrap_type_layers(ty: &syn::Type) -> &syn::Type {
    match ty {
        syn::Type::Reference(r) => unwrap_type_layers(&r.elem),
        syn::Type::Paren(p) => unwrap_type_layers(&p.elem),
        syn::Type::Group(g) => unwrap_type_layers(&g.elem),
        other => other,
    }
}

/// True if `im` is a MANUAL trait impl (`impl Trait for Carrier`) whose
/// `self_ty` is one of the carrier structs — the shape forbidden in carrier.rs.
/// `#[derive(...)]`-generated impls are ATTRIBUTES, not `syn::Item::Impl`
/// blocks, so they are never seen here; only hand-written trait impls are. An
/// inherent `impl Carrier { … }` (no `trait_`) returns false, as does a trait
/// impl on a non-carrier struct.
///
/// The self-ty is UNWRAPPED ([`unwrap_type_layers`]) before the carrier-name
/// check, so `impl AsRef<…> for &BareRefCarrier` (reference self-ty),
/// `impl … for (BareRefCarrier)` (parenthesised), and the macro-`Group`-wrapped
/// form are all recognised as trait impls on the carrier.
fn impl_is_manual_trait_on_carrier(im: &syn::ItemImpl) -> bool {
    if im.trait_.is_none() {
        return false;
    }
    match type_path_last_ident(unwrap_type_layers(&im.self_ty)) {
        Some(name) => CARRIER_STRUCTS.contains(&name.as_str()),
        None => false,
    }
}

// ════════════════════════════════════════════════════════════════════
// EXACT-SHAPE allowlist scan.
//
// carrier.rs is the SOLE module that can reach a carrier's raw args, so the
// file-scoped guard is COMPLETE only if it polices the WHOLE module shape. The
// scan is a STRICT EXACT-SHAPE ALLOWLIST: every item is matched against its
// expected entry in the description tables above; an item that is not described
// (any extra struct / enum / trait / union / fn / macro / nested mod / …), a
// described item with the wrong shape (renamed import, off-spec head alias,
// changed field/method signature, non-private member, missing/extra accessor),
// a macro in any body, or a raw-args read outside the sanctioned bodies all
// produce a violation; a described item that is MISSING also produces one. The
// predicates are factored so the `*_discriminates` self-tests can feed
// synthetic / mutated inputs and prove each rule rejects its bypass.
// ════════════════════════════════════════════════════════════════════

/// Normalize any `ToTokens` value to syn's canonical spaced token string, for
/// exact whitespace-insensitive comparison.
fn norm_tokens<T: ToTokens>(t: &T) -> String {
    t.to_token_stream().to_string()
}

/// Normalize an EXPECTED snippet by parsing it through the same syn grammar then
/// re-emitting, so it shares the exact spacing of the actual side.
fn norm_expected<T: syn::parse::Parse + ToTokens>(text: &str) -> String {
    let parsed: T = syn::parse_str(text)
        .unwrap_or_else(|e| panic!("expected snippet `{text}` must parse: {e}"));
    norm_tokens(&parsed)
}

/// Canonical string of a method signature, reconstructed from its parts so a
/// rustfmt trailing comma in a multi-line param list (`…,) -> Self`) does not
/// cause a spurious mismatch: `sig.inputs.iter()` yields each parameter WITHOUT
/// the trailing punctuation. Both the actual and the expected signature run
/// through this same builder, so the comparison is whitespace- and
/// trailing-comma-insensitive but otherwise EXACT (ident, generics, each
/// parameter, return type, where-clause).
fn canonical_sig(sig: &syn::Signature) -> String {
    let inputs = sig
        .inputs
        .iter()
        .map(norm_tokens)
        .collect::<Vec<_>>()
        .join(" , ");
    let generics = norm_tokens(&sig.generics);
    let output = match &sig.output {
        syn::ReturnType::Default => String::new(),
        syn::ReturnType::Type(_, t) => format!(" -> {}", norm_tokens(t.as_ref())),
    };
    let where_clause = sig
        .generics
        .where_clause
        .as_ref()
        .map(norm_tokens)
        .unwrap_or_default();
    format!(
        "fn {}{generics} ({inputs}){output}{where_clause}",
        sig.ident
    )
}

/// Parse an EXPECTED signature snippet, panicking with context on failure.
fn parse_sig(text: &str) -> syn::Signature {
    syn::parse_str(text).unwrap_or_else(|e| panic!("expected signature `{text}` must parse: {e}"))
}

/// Dotted ident path of a `syn::Path` for messages (`serde::Serialize` →
/// `"serde::Serialize"`).
fn path_to_string(p: &syn::Path) -> String {
    p.segments
        .iter()
        .map(|s| s.ident.to_string())
        .collect::<Vec<_>>()
        .join("::")
}

/// Human-readable name of a `syn::Item` kind, for the allowlist catch-all's
/// rejection message.
fn item_kind_name(item: &syn::Item) -> &'static str {
    match item {
        syn::Item::Const(_) => "const",
        syn::Item::Enum(_) => "enum",
        syn::Item::ExternCrate(_) => "extern crate",
        syn::Item::Fn(_) => "free fn",
        syn::Item::ForeignMod(_) => "extern block (foreign mod)",
        syn::Item::Impl(_) => "impl",
        syn::Item::Macro(_) => "item macro",
        syn::Item::Mod(_) => "nested module",
        syn::Item::Static(_) => "static",
        syn::Item::Struct(_) => "struct",
        syn::Item::Trait(_) => "trait",
        syn::Item::TraitAlias(_) => "trait alias",
        syn::Item::Type(_) => "type alias",
        syn::Item::Union(_) => "union",
        syn::Item::Use(_) => "use import",
        syn::Item::Verbatim(_) => "verbatim token block",
        _ => "unrecognised item",
    }
}

/// Reject any attribute on `owner` whose (single-segment) path ident is not in
/// `allowed`. Closes the `#[cfg]` / `#[cfg_attr]` / foreign attribute-macro
/// holes uniformly: a conditional attribute could swap the item out, and an
/// attribute proc-macro could rewrite it — neither is visible to a source scan.
/// A multi-segment attribute path is always foreign and rejected.
fn check_attrs(attrs: &[syn::Attribute], allowed: &[&str], owner: &str, v: &mut Vec<String>) {
    for attr in attrs {
        let p = attr.path();
        let single = p.segments.len() == 1;
        let name = p
            .segments
            .last()
            .map(|s| s.ident.to_string())
            .unwrap_or_default();
        if !(single && allowed.contains(&name.as_str())) {
            v.push(format!(
                "attribute `#[{}]` on {owner} is forbidden in carrier.rs — only {allowed:?} \
                 attributes are permitted (a foreign / conditional attribute could rewrite or swap \
                 the item the scan cannot see)",
                path_to_string(p)
            ));
        }
    }
}

/// POINT 1. The `mod carrier;` declaration in `semantic_query.rs` must be
/// UNADORNED: an out-of-line `pub mod carrier;` (or `mod carrier;`) with NO
/// `#[path]` redirect, NO `#[cfg]` / `#[cfg_attr]` indirection, and NO inline
/// `mod carrier { … }` body. Any of those could move or conditionally swap the
/// carrier source out from under the file-scoped shape scan.
fn carrier_module_decl_violations(file: &syn::File) -> Vec<String> {
    let mut v = Vec::new();
    let mut found = false;
    for item in &file.items {
        let syn::Item::Mod(m) = item else { continue };
        if m.ident != "carrier" {
            continue;
        }
        found = true;
        if m.content.is_some() {
            v.push(
                "`mod carrier` must be an out-of-line declaration (`pub mod carrier;`), not an \
                 inline `mod carrier { … }` — an inline body would move the carrier surface out of \
                 the policed carrier.rs file"
                    .to_string(),
            );
        }
        for attr in &m.attrs {
            if attr.path().is_ident("doc") {
                continue;
            }
            v.push(format!(
                "`mod carrier` must be UNADORNED — attribute `#[{}]` (e.g. `#[path]` / `#[cfg]` / \
                 `#[cfg_attr]`) is forbidden: it could redirect or conditionally swap the carrier \
                 source the guard scans",
                path_to_string(attr.path())
            ));
        }
    }
    if !found {
        v.push(format!(
            "`mod carrier;` declaration not found in {SEMANTIC_QUERY_RS} — the carrier module must \
             be declared as an unadorned `pub mod carrier;`"
        ));
    }
    v
}

/// POINTS 3-11. EXACT-SHAPE scan of a carrier.rs `syn::File`. Returns one
/// message per deviation; an empty result means the module matches the precise
/// known-good shape exactly.
fn carrier_module_shape_violations(file: &syn::File) -> Vec<String> {
    let mut v = Vec::new();
    let mut seen_uses: Vec<String> = Vec::new();
    let mut seen_aliases: BTreeSet<String> = BTreeSet::new();
    let mut seen_structs: BTreeSet<String> = BTreeSet::new();
    let mut seen_carrier_impls: BTreeSet<String> = BTreeSet::new();
    let mut seen_semantic_impl = false;

    for item in &file.items {
        match item {
            syn::Item::Use(u) => check_use(u, &mut seen_uses, &mut v),
            syn::Item::Type(t) => check_head_alias(t, &mut seen_aliases, &mut v),
            syn::Item::Struct(s) => check_struct(s, &mut seen_structs, &mut v),
            syn::Item::Impl(im) => {
                check_impl(im, &mut seen_carrier_impls, &mut seen_semantic_impl, &mut v)
            }

            // ── REJECTED, with a tailored message for the common leak vectors ──
            syn::Item::Mod(m) => v.push(format!(
                "nested module `mod {}` is forbidden in carrier.rs — a submodule could synthesise a \
                 leaking carrier surface this file-scoped guard does not police",
                m.ident
            )),
            syn::Item::Macro(mac) => {
                let name = mac
                    .mac
                    .path
                    .segments
                    .last()
                    .map(|s| s.ident.to_string())
                    .unwrap_or_default();
                v.push(format!(
                    "item-level macro invocation `{name}!` is forbidden in carrier.rs — a macro \
                     (e.g. `include!`) could expand to a leaking method or impl"
                ));
            }
            syn::Item::Fn(f) => v.push(format!(
                "free fn `{}` is forbidden in carrier.rs — only the sanctioned `impl \
                 SemanticNodeData` accessors and the PRIVATE carrier methods may exist",
                f.sig.ident
            )),
            syn::Item::Static(s) => v.push(format!(
                "`static {}` is forbidden in carrier.rs — the module holds only the carriers + \
                 their accessor surface",
                s.ident
            )),
            syn::Item::Const(c) => v.push(format!(
                "`const {}` is forbidden in carrier.rs — the module holds only the carriers + \
                 their accessor surface",
                c.ident
            )),

            // ── ALLOWLIST CATCH-ALL: every OTHER item kind REJECTS (no silent
            // accept). `trait` / `union` / `enum` / `trait alias` /
            // `extern crate` / `extern block` / `verbatim` / any future
            // `syn::Item` variant all land here. ──
            other => v.push(format!(
                "a `{}` is forbidden in carrier.rs — the module may contain ONLY the two sanctioned \
                 `use` imports, the three head-view aliases, the three carrier structs, their \
                 private inherent impls, and the `impl SemanticNodeData` accessor block",
                item_kind_name(other)
            )),
        }
    }

    // ── Required-presence checks: a described item that is MISSING is a
    // deviation just like an unexpected one. ──
    for expected in EXPECTED_USES {
        let want = norm_expected::<syn::ItemUse>(expected);
        if !seen_uses.iter().any(|s| *s == want) {
            v.push(format!(
                "required `use` import `{expected}` is missing from carrier.rs"
            ));
        }
    }
    for (name, _) in EXPECTED_HEAD_ALIASES {
        if !seen_aliases.contains(name) {
            v.push(format!(
                "required head-view alias `{name}` is missing from carrier.rs"
            ));
        }
    }
    for spec in &CARRIER_SPECS {
        if !seen_structs.contains(spec.name) {
            v.push(format!(
                "required carrier struct `{}` is missing from carrier.rs",
                spec.name
            ));
        }
        if !seen_carrier_impls.contains(spec.name) {
            v.push(format!(
                "required inherent `impl {}` is missing from carrier.rs",
                spec.name
            ));
        }
    }
    if !seen_semantic_impl {
        v.push(
            "required inherent `impl SemanticNodeData` accessor block is missing from carrier.rs"
                .to_string(),
        );
    }

    v
}

/// Each `use` must be EXACTLY one of the two sanctioned imports — no rename
/// (`use … as …`), no extra, no `pub use` re-export.
fn check_use(u: &syn::ItemUse, seen: &mut Vec<String>, v: &mut Vec<String>) {
    check_attrs(&u.attrs, &["doc"], "a `use` import", v);
    if use_tree_has_rename(&u.tree) {
        v.push(
            "an import rename (`use … as …`) is forbidden in carrier.rs — a renamed import could \
             re-introduce a bindable args type/path under a different name"
                .to_string(),
        );
    }
    let mut bare = u.clone();
    bare.attrs.clear();
    let norm = norm_tokens(&bare);
    seen.push(norm.clone());
    let permitted = EXPECTED_USES
        .iter()
        .any(|e| norm_expected::<syn::ItemUse>(e) == norm);
    if !permitted {
        v.push(format!(
            "unexpected `use` import `{norm}` in carrier.rs — only the exact set {EXPECTED_USES:?} \
             (no renames, no extras) is permitted"
        ));
    }
}

/// True iff a use-tree contains any `use … as …` rename, anywhere in a grouped
/// or pathed tree.
fn use_tree_has_rename(tree: &syn::UseTree) -> bool {
    match tree {
        syn::UseTree::Rename(_) => true,
        syn::UseTree::Path(p) => use_tree_has_rename(&p.tree),
        syn::UseTree::Group(g) => g.items.iter().any(use_tree_has_rename),
        _ => false,
    }
}

/// Each `type` alias must be one of the three sanctioned head-view aliases, by
/// its EXACT definition (attrs ignored). A non-sanctioned name, or a sanctioned
/// name with a different RHS, is a deviation.
fn check_head_alias(t: &syn::ItemType, seen: &mut BTreeSet<String>, v: &mut Vec<String>) {
    let name = t.ident.to_string();
    check_attrs(&t.attrs, &["doc"], &format!("type alias `{name}`"), v);
    let Some((_, expected)) = EXPECTED_HEAD_ALIASES.iter().find(|(n, _)| *n == name) else {
        v.push(format!(
            "type alias `{name}` is forbidden in carrier.rs — only the three sanctioned head-view \
             aliases ({:?}) may exist",
            head_alias_names()
        ));
        return;
    };
    seen.insert(name.clone());
    let mut bare = t.clone();
    bare.attrs.clear();
    let actual = norm_tokens(&bare);
    if actual != norm_expected::<syn::ItemType>(expected) {
        v.push(format!(
            "head alias `{name}` must match its exact sanctioned definition `{expected}` — found \
             `{actual}` (a different RHS could expose the carrier's args or alias the opaque \
             payload)"
        ));
    }
}

/// Each `struct` must be one of the three carriers, with the EXACT private field
/// set and EXACTLY the five built-in derives. A non-carrier (helper) struct is a
/// deviation — even one that itself exposes a `type_args` field.
fn check_struct(s: &syn::ItemStruct, seen: &mut BTreeSet<String>, v: &mut Vec<String>) {
    let name = s.ident.to_string();
    let Some(spec) = CARRIER_SPECS.iter().find(|c| c.name == name) else {
        v.push(format!(
            "struct `{name}` is forbidden in carrier.rs — the module defines ONLY the three carrier \
             structs ({CARRIER_STRUCTS:?}); a helper struct (e.g. one exposing `type_args`) is a \
             deviation"
        ));
        return;
    };
    seen.insert(name.clone());
    // Only `#[derive(...)]` + doc comments are permitted; the derive CONTENTS
    // are then validated to be exactly the five built-ins.
    check_attrs(
        &s.attrs,
        &["derive", "doc"],
        &format!("carrier struct `{name}`"),
        v,
    );
    check_derive_exact(&s.attrs, &name, v);
    check_struct_fields(s, spec, v);
}

/// The carrier struct must derive EXACTLY [`ALLOWED_DERIVES`] — by bare,
/// single-segment built-in idents. A qualified derive path (`foo::Clone`), a
/// custom derive, or an extra/missing built-in is a deviation.
fn check_derive_exact(attrs: &[syn::Attribute], owner: &str, v: &mut Vec<String>) {
    let mut derives: Vec<String> = Vec::new();
    for attr in attrs {
        if !attr.path().is_ident("derive") {
            continue;
        }
        let _ = attr.parse_nested_meta(|meta| {
            if meta.path.segments.len() != 1 {
                v.push(format!(
                    "qualified derive path `{}` on `{owner}` is forbidden — derives must be the \
                     bare built-ins {ALLOWED_DERIVES:?} (a qualified path could resolve to a custom \
                     derive synthesising a leaking trait impl)",
                    path_to_string(&meta.path)
                ));
            } else {
                derives.push(meta.path.segments[0].ident.to_string());
            }
            Ok(())
        });
    }
    let actual: BTreeSet<&str> = derives.iter().map(String::as_str).collect();
    let expected: BTreeSet<&str> = ALLOWED_DERIVES.iter().copied().collect();
    if actual != expected {
        v.push(format!(
            "carrier struct `{owner}` must derive EXACTLY {ALLOWED_DERIVES:?} — found {derives:?} \
             (an extra derive could attach a leaking trait surface; a missing one changes the \
             frozen shape)"
        ));
    }
}

/// The carrier struct's fields must be EXACTLY `spec.fields` (ordered names +
/// types), all PRIVATE. An extra / missing / renamed / retyped field, or any
/// non-private field, is a deviation.
fn check_struct_fields(s: &syn::ItemStruct, spec: &CarrierSpec, v: &mut Vec<String>) {
    let syn::Fields::Named(named) = &s.fields else {
        v.push(format!(
            "carrier struct `{}` must have named fields",
            spec.name
        ));
        return;
    };
    let mut actual: Vec<(String, String)> = Vec::new();
    for field in &named.named {
        let fname = field
            .ident
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_default();
        if !matches!(field.vis, syn::Visibility::Inherited) {
            v.push(format!(
                "carrier field `{}::{fname}` must be PRIVATE — a visible field re-opens the \
                 `node.{fname}` anti-tail bind opacity prevents",
                spec.name
            ));
        }
        actual.push((fname, norm_tokens(&field.ty)));
    }
    let expected: Vec<(String, String)> = spec
        .fields
        .iter()
        .map(|(n, t)| (n.to_string(), norm_expected::<syn::Type>(t)))
        .collect();
    if actual != expected {
        v.push(format!(
            "carrier struct `{}` field set must be EXACTLY {:?} — found {actual:?}",
            spec.name, spec.fields
        ));
    }
}

/// Each `impl` must be an inherent impl on one of the three carriers or on
/// `SemanticNodeData`. A trait impl (which would hand the raw args out through a
/// crate-wide trait method), an impl on an unrelated type, or a duplicate impl
/// is a deviation. The self-ty is UNWRAPPED first so a reference self-ty
/// (`impl … for &BareRefCarrier`) is classified correctly.
fn check_impl(
    im: &syn::ItemImpl,
    seen_carrier_impls: &mut BTreeSet<String>,
    seen_semantic_impl: &mut bool,
    v: &mut Vec<String>,
) {
    check_attrs(&im.attrs, &["doc"], "an impl block", v);

    let self_name = type_path_last_ident(unwrap_type_layers(&im.self_ty));
    let label = self_name.as_deref().unwrap_or("<non-path-type>");

    if im.trait_.is_some() {
        if impl_is_manual_trait_on_carrier(im) {
            v.push(format!(
                "manual trait impl on carrier struct `{label}` is forbidden — a trait method (e.g. \
                 `AsRef<[SemanticNodeId]>` / `Deref` / `Borrow`) could leak the raw args \
                 crate-wide; carriers obtain traits via `#[derive(...)]` ONLY"
            ));
        } else if label == "SemanticNodeData" {
            v.push(
                "trait impl on `SemanticNodeData` is forbidden in carrier.rs — only the inherent \
                 accessor block may exist; a trait method could expose the raw args crate-wide"
                    .to_string(),
            );
        } else {
            v.push(format!(
                "trait impl on `{label}` is forbidden in carrier.rs — the module carries only \
                 inherent impls on the carrier structs and `SemanticNodeData`"
            ));
        }
        return;
    }

    if let Some(spec) = CARRIER_SPECS.iter().find(|c| c.name == label) {
        if !seen_carrier_impls.insert(label.to_string()) {
            v.push(format!(
                "duplicate inherent `impl {label}` — exactly one inherent impl per carrier is \
                 permitted"
            ));
        }
        check_carrier_impl(im, spec, v);
    } else if label == "SemanticNodeData" {
        if *seen_semantic_impl {
            v.push(
                "duplicate inherent `impl SemanticNodeData` — exactly one accessor block is \
                 permitted"
                    .to_string(),
            );
        }
        *seen_semantic_impl = true;
        check_semantic_impl(im, v);
    } else {
        v.push(format!(
            "inherent impl on `{label}` is forbidden in carrier.rs — the module carries only the \
             carrier structs' impls and the `SemanticNodeData` accessor block"
        ));
    }
}

/// A carrier's inherent impl must contain EXACTLY its `spec.methods` (names +
/// signatures), every method PRIVATE, every body macro-free and (outside the
/// sanctioned descent/rebuild set) free of raw-args reads. An extra / missing /
/// retyped / non-private method, an impl-item macro, or a non-fn impl item is a
/// deviation.
fn check_carrier_impl(im: &syn::ItemImpl, spec: &CarrierSpec, v: &mut Vec<String>) {
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for ii in &im.items {
        match ii {
            syn::ImplItem::Fn(f) => {
                let name = f.sig.ident.to_string();
                seen.insert(name.clone());
                check_attrs(&f.attrs, &["doc"], &format!("`{}::{name}`", spec.name), v);
                if vis_escapes_module(&f.vis) {
                    v.push(format!(
                        "carrier method `{}::{name}` must be PRIVATE (no `pub` / `pub(crate)` / \
                         `pub(super)`) — the raw-args surface is confined to carrier.rs",
                        spec.name
                    ));
                }
                match spec.methods.iter().find(|(n, _)| *n == name) {
                    Some((_, sig)) => {
                        let actual = canonical_sig(&f.sig);
                        if actual != canonical_sig(&parse_sig(sig)) {
                            v.push(format!(
                                "carrier method `{}::{name}` signature must be EXACTLY `{sig}` — \
                                 found `{actual}`",
                                spec.name
                            ));
                        }
                    }
                    None => v.push(format!(
                        "unexpected method `{}::{name}` — the carrier's inherent impl may carry \
                         ONLY {:?}",
                        spec.name,
                        method_names(spec)
                    )),
                }
                scan_method_body(&name, &f.block, v);
            }
            syn::ImplItem::Macro(_) => v.push(format!(
                "macro invocation inside `impl {}` is forbidden in carrier.rs — it could expand to \
                 a leaking method",
                spec.name
            )),
            _ => v.push(format!(
                "unexpected item inside `impl {}` — only the sanctioned private methods may exist",
                spec.name
            )),
        }
    }
    for (n, _) in spec.methods {
        if !seen.contains(*n) {
            v.push(format!(
                "carrier impl `{}` is missing required method `{n}`",
                spec.name
            ));
        }
    }
}

/// The `impl SemanticNodeData` accessor block must contain EXACTLY the eight
/// sanctioned accessors ([`ACCESSOR_SPECS`]) — each at its exact visibility and
/// exact signature, every body macro-free and (outside the sanctioned set) free
/// of raw-args reads. An extra / missing / renamed accessor, a visibility drift,
/// a signature drift, an impl-item macro, or a non-fn impl item is a deviation.
fn check_semantic_impl(im: &syn::ItemImpl, v: &mut Vec<String>) {
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for ii in &im.items {
        match ii {
            syn::ImplItem::Fn(f) => {
                let name = f.sig.ident.to_string();
                seen.insert(name.clone());
                check_attrs(
                    &f.attrs,
                    &["doc", "must_use"],
                    &format!("`SemanticNodeData::{name}`"),
                    v,
                );
                match ACCESSOR_SPECS.iter().find(|a| a.name == name) {
                    Some(spec) => {
                        let actual_vis = norm_tokens(&f.vis);
                        if actual_vis != norm_expected::<syn::Visibility>(spec.vis) {
                            v.push(format!(
                                "accessor `{name}` must have visibility `{}` — found `{actual_vis}`",
                                spec.vis
                            ));
                        }
                        let actual_sig = canonical_sig(&f.sig);
                        if actual_sig != canonical_sig(&parse_sig(spec.sig)) {
                            v.push(format!(
                                "accessor `{name}` signature must be EXACTLY `{}` — found \
                                 `{actual_sig}`",
                                spec.sig
                            ));
                        }
                    }
                    None => v.push(format!(
                        "unexpected method `SemanticNodeData::{name}` — the accessor block may carry \
                         ONLY the eight sanctioned accessors {:?}",
                        accessor_names()
                    )),
                }
                scan_method_body(&name, &f.block, v);
            }
            syn::ImplItem::Macro(_) => v.push(
                "macro invocation inside `impl SemanticNodeData` is forbidden in carrier.rs — it \
                 could expand to a leaking accessor"
                    .to_string(),
            ),
            _ => v.push(
                "unexpected item inside `impl SemanticNodeData` — only the eight sanctioned methods \
                 may exist"
                    .to_string(),
            ),
        }
    }
    for spec in &ACCESSOR_SPECS {
        if !seen.contains(spec.name) {
            v.push(format!(
                "`impl SemanticNodeData` is missing sanctioned accessor `{}`",
                spec.name
            ));
        }
    }
}

/// Body-level checks (POINT 11). Ban any macro invocation in the body (a macro
/// could synthesise a raw-args read/leak the scan cannot see), and ban any raw
/// `.type_args` field read / `.arg_nodes()` method call OUTSIDE the sanctioned
/// descent/rebuild bodies ([`SANCTIONED_RAW_BODIES`]).
fn scan_method_body(method: &str, block: &syn::Block, v: &mut Vec<String>) {
    if body_has_macro(block) {
        v.push(format!(
            "method `{method}` body contains a macro invocation — forbidden in carrier.rs (a macro \
             could synthesise a raw-args read the scan cannot see)"
        ));
    }
    if !SANCTIONED_RAW_BODIES.contains(&method) {
        if let Some(read) = body_raw_args_read(block) {
            v.push(format!(
                "method `{method}` reads `{read}` outside the sanctioned descent/rebuild bodies \
                 ({SANCTIONED_RAW_BODIES:?}) — only those may touch a carrier's raw args"
            ));
        }
    }
}

/// True iff `block` contains any macro invocation (expression / statement /
/// nested item macro), found via `syn::visit::visit_macro`.
fn body_has_macro(block: &syn::Block) -> bool {
    struct MacroFinder {
        found: bool,
    }
    impl<'ast> syn::visit::Visit<'ast> for MacroFinder {
        fn visit_macro(&mut self, _m: &'ast syn::Macro) {
            self.found = true;
        }
    }
    let mut f = MacroFinder { found: false };
    syn::visit::Visit::visit_block(&mut f, block);
    f.found
}

/// First raw-args READ in `block`, or `None`. A read is a `.type_args` field
/// access (`syn::ExprField` with named member `type_args`) or an `.arg_nodes()`
/// method call (`syn::ExprMethodCall`). A struct-literal field-init shorthand
/// (`Self { type_args }`) and a bare parameter ident `type_args` are NOT field
/// reads, so construction / rebuild bodies are not falsely flagged.
fn body_raw_args_read(block: &syn::Block) -> Option<String> {
    struct RawFinder {
        found: Option<String>,
    }
    impl<'ast> syn::visit::Visit<'ast> for RawFinder {
        fn visit_expr_field(&mut self, e: &'ast syn::ExprField) {
            if self.found.is_none() {
                if let syn::Member::Named(id) = &e.member {
                    if id == "type_args" {
                        self.found = Some(".type_args".to_string());
                    }
                }
            }
            syn::visit::visit_expr_field(self, e);
        }
        fn visit_expr_method_call(&mut self, e: &'ast syn::ExprMethodCall) {
            if self.found.is_none() && e.method == "arg_nodes" {
                self.found = Some(".arg_nodes()".to_string());
            }
            syn::visit::visit_expr_method_call(self, e);
        }
    }
    let mut f = RawFinder { found: None };
    syn::visit::Visit::visit_block(&mut f, block);
    f.found
}

/// The sanctioned head-view alias names, for the rejection message.
fn head_alias_names() -> Vec<&'static str> {
    EXPECTED_HEAD_ALIASES.iter().map(|(n, _)| *n).collect()
}

/// A carrier spec's method names, for the rejection message.
fn method_names(spec: &CarrierSpec) -> Vec<&'static str> {
    spec.methods.iter().map(|(n, _)| *n).collect()
}

/// The eight sanctioned accessor names, for the rejection message.
fn accessor_names() -> Vec<&'static str> {
    ACCESSOR_SPECS.iter().map(|a| a.name).collect()
}

/// True if a match-arm pattern is an irrefutable catch-all. The
/// detector RECURSIVELY UNWRAPS the layers that can hide one:
///   - `_` (`Pat::Wild`);
///   - a bare binding `other` (`Pat::Ident` with no subpattern) — matches
///     everything; and `x @ <sub>` is a catch-all iff `<sub>` is;
///   - a `|`-pattern (`Pat::Or`) any of whose alternatives is a catch-all;
///   - a parenthesised `(_)` (`Pat::Paren`) or a reference `&_` / `&mut _`
///     (`Pat::Reference`).
///
/// (syn 2.x has no `Pat::Group`; a grouping pattern surfaces as `Pat::Paren`, so
/// unwrapping `Paren`/`Reference`/`Or`/`Ident`-subpat covers `(_)`, `&_`,
/// `x @ _`, and `A | _`.) A qualified path / tuple-struct / struct pattern
/// naming a variant (`Self::Foo(..)`, `Foo { .. }`) is NOT a catch-all.
fn arm_is_catchall(pat: &syn::Pat) -> bool {
    match pat {
        syn::Pat::Wild(_) => true,
        syn::Pat::Ident(pi) => match &pi.subpat {
            None => true,
            Some((_, sub)) => arm_is_catchall(sub),
        },
        syn::Pat::Or(po) => po.cases.iter().any(arm_is_catchall),
        syn::Pat::Paren(pp) => arm_is_catchall(&pp.pat),
        syn::Pat::Reference(pr) => arm_is_catchall(&pr.pat),
        _ => false,
    }
}

// ── synthetic-snippet parse helpers (self-test fixtures only) ──────────

/// Parse a single enum variant from its source text (wrapped in a throwaway
/// enum so we exercise syn's real variant grammar).
fn parse_variant(variant_src: &str) -> syn::Variant {
    let wrapped = format!("enum E {{ {variant_src} }}");
    let en: syn::ItemEnum =
        syn::parse_str(&wrapped).unwrap_or_else(|e| panic!("parse variant `{variant_src}`: {e}"));
    en.variants
        .into_iter()
        .next()
        .expect("one synthetic variant")
}

/// Parse a single impl method from its source text (wrapped in a throwaway
/// inherent impl).
fn parse_impl_method(method_src: &str) -> syn::ImplItemFn {
    let wrapped = format!("impl S {{ {method_src} }}");
    let im: syn::ItemImpl =
        syn::parse_str(&wrapped).unwrap_or_else(|e| panic!("parse method `{method_src}`: {e}"));
    im.items
        .into_iter()
        .find_map(|i| match i {
            syn::ImplItem::Fn(f) => Some(f),
            _ => None,
        })
        .expect("one synthetic method")
}

/// Parse a whole `impl` block (inherent OR `impl Trait for Type`) from source.
fn parse_impl_block(impl_src: &str) -> syn::ItemImpl {
    syn::parse_str(impl_src).unwrap_or_else(|e| panic!("parse impl `{impl_src}`: {e}"))
}

/// Parse the arms of a synthetic `match` expression.
fn parse_match_arms(match_src: &str) -> Vec<syn::Arm> {
    let expr: syn::Expr =
        syn::parse_str(match_src).unwrap_or_else(|e| panic!("parse match `{match_src}`: {e}"));
    match expr {
        syn::Expr::Match(m) => m.arms,
        _ => panic!("`{match_src}` is not a match expression"),
    }
}

/// Parse a single attribute (e.g. `#[derive(Default)]`) by attaching it to a
/// throwaway struct and taking the first attr.
fn parse_attr(attr_src: &str) -> syn::Attribute {
    let item: syn::ItemStruct = syn::parse_str(&format!("{attr_src}\nstruct S;"))
        .unwrap_or_else(|e| panic!("parse attr `{attr_src}`: {e}"));
    item.attrs.into_iter().next().expect("one synthetic attr")
}

/// Parse a single top-level item (e.g. an extra struct / use / mod) from source.
fn parse_item(item_src: &str) -> syn::Item {
    syn::parse_str(item_src).unwrap_or_else(|e| panic!("parse item `{item_src}`: {e}"))
}

/// Parse a single statement (e.g. one carrying a macro) from source.
fn parse_stmt(stmt_src: &str) -> syn::Stmt {
    syn::parse_str(stmt_src).unwrap_or_else(|e| panic!("parse stmt `{stmt_src}`: {e}"))
}

/// The real carrier.rs parsed into a mutable AST — the base for mutation-based
/// self-tests (asserted to scan CLEAN before any mutation).
fn real_carrier_file() -> syn::File {
    parse(CARRIER_RS)
}

/// Find the named carrier struct in a parsed file, mutably.
fn find_struct_mut<'a>(file: &'a mut syn::File, name: &str) -> &'a mut syn::ItemStruct {
    file.items
        .iter_mut()
        .find_map(|it| match it {
            syn::Item::Struct(s) if s.ident == name => Some(s),
            _ => None,
        })
        .unwrap_or_else(|| panic!("carrier struct `{name}` not found"))
}

/// Find the inherent `impl <self_name>` block in a parsed file, mutably.
fn find_impl_mut<'a>(file: &'a mut syn::File, self_name: &str) -> &'a mut syn::ItemImpl {
    file.items
        .iter_mut()
        .find_map(|it| match it {
            syn::Item::Impl(im)
                if im.trait_.is_none()
                    && type_path_last_ident(&im.self_ty).as_deref() == Some(self_name) =>
            {
                Some(im)
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("inherent impl `{self_name}` not found"))
}

/// Find the named method in an impl block, mutably.
fn find_method_mut<'a>(im: &'a mut syn::ItemImpl, name: &str) -> &'a mut syn::ImplItemFn {
    im.items
        .iter_mut()
        .find_map(|ii| match ii {
            syn::ImplItem::Fn(f) if f.sig.ident == name => Some(f),
            _ => None,
        })
        .unwrap_or_else(|| panic!("method `{name}` not found"))
}

/// SELF-TEST. The payload check requires the FULL two-segment
/// path `carrier::{Name}Carrier`, rejecting THREE weak-form attacks a
/// final-segment-only check (a weaker form) would have let through, plus the
/// raw-tuple attack: (A) an UNQUALIFIED `BareRef(BareRefCarrier)` — the shape a
/// raw `type BareRefCarrier = Arc<[SemanticNodeId]>` alias takes (a syn scan
/// cannot resolve the alias, so the `carrier::` qualifier is the defense); (B) a
/// WRONG-MODULE `BareRef(other::BareRefCarrier)` whose final segment matches but
/// whose payload is not the sealed carrier; (C) a raw `BareRef(Arc<…>)`.
/// Reverting `carrier_variant_payload_is_opaque` to a final-segment-only check
/// makes attacks (A) and (B) pass — these assertions then FAIL.
#[test]
fn carrier_variant_payload_type_check_discriminates() {
    // Opaque payload — ACCEPTED (full path is exactly `carrier::BareRefCarrier`).
    let good = parse_variant("BareRef(carrier::BareRefCarrier)");
    assert_eq!(
        carrier_variant_payload_type_path(&good).as_deref(),
        Some(["carrier".to_string(), "BareRefCarrier".to_string()].as_slice()),
        "the opaque payload's full type path must read as `carrier::BareRefCarrier`"
    );
    assert!(
        carrier_variant_payload_is_opaque(&good, "BareRefCarrier"),
        "the opaque `carrier::BareRefCarrier` payload is accepted"
    );

    // Attack A: UNQUALIFIED `BareRef(BareRefCarrier)` — a single-segment path,
    // the shape a raw `type BareRefCarrier = Arc<[SemanticNodeId]>` alias would
    // take. REJECTED (the final-segment-only weak form passed it).
    let unqualified = parse_variant("BareRef(BareRefCarrier)");
    assert_eq!(
        carrier_variant_payload_type_path(&unqualified).as_deref(),
        Some(["BareRefCarrier".to_string()].as_slice()),
    );
    assert!(
        !carrier_variant_payload_is_opaque(&unqualified, "BareRefCarrier"),
        "DISCRIMINATION: an UNQUALIFIED `BareRef(BareRefCarrier)` (no `carrier::` qualifier — the \
         shape a raw `type BareRefCarrier = Arc<[SemanticNodeId]>` alias takes) must be REJECTED; \
         the final-segment-only weak form passed it."
    );

    // Attack B: WRONG-MODULE `BareRef(other::BareRefCarrier)` — final segment
    // matches but the module is not `carrier`. REJECTED (the final-segment-only
    // weak form passed it).
    let wrong_module = parse_variant("BareRef(other::BareRefCarrier)");
    assert_eq!(
        carrier_variant_payload_type_path(&wrong_module).as_deref(),
        Some(["other".to_string(), "BareRefCarrier".to_string()].as_slice()),
    );
    assert!(
        !carrier_variant_payload_is_opaque(&wrong_module, "BareRefCarrier"),
        "DISCRIMINATION: a WRONG-MODULE `BareRef(other::BareRefCarrier)` must be REJECTED — its \
         final segment matches `BareRefCarrier`, so the final-segment-only weak form passed it."
    );

    // Attack C: raw-tuple / alias payload `Arc<[SemanticNodeId]>` — single-field
    // tuple (passes arity) but its path is `Arc`. REJECTED.
    let raw = parse_variant("BareRef(Arc<[SemanticNodeId]>)");
    assert_eq!(
        carrier_variant_payload_type_path(&raw).as_deref(),
        Some(["Arc".to_string()].as_slice()),
    );
    assert!(
        !carrier_variant_payload_is_opaque(&raw, "BareRefCarrier"),
        "DISCRIMINATION: a raw-tuple payload `BareRef(Arc<[SemanticNodeId]>)` must be REJECTED — \
         this is the positional `type_args`-binding hole the arity-only tripwire let through."
    );

    // The OPAQUE payload is the ONLY accepted shape.
    let named = parse_variant("BareRef { name: Arc<str>, type_args: Arc<[SemanticNodeId]> }");
    assert_eq!(
        carrier_variant_payload_type_path(&named),
        None,
        "a named-struct variant exposes no single opaque payload path"
    );
    assert!(
        !carrier_variant_payload_is_opaque(&named, "BareRefCarrier"),
        "a named-struct variant is rejected by the opaque-payload predicate"
    );
}

/// SELF-TEST for the EXACT-SHAPE allowlist ([`carrier_module_shape_violations`]).
/// Proves the strict allowlist ACCEPTS the real sealed carrier.rs and REJECTS
/// every known bypass — each applied as ONE targeted mutation of
/// the real AST, so the test stays faithful to whatever the real shape is. Each
/// mutation hits a distinct rule; reverting that rule to a weaker form makes the
/// matching assertion FAIL (red→green proof).
#[test]
fn carrier_exact_shape_allowlist_discriminates() {
    // The REAL sealed carrier.rs scans CLEAN under the exact-shape allowlist.
    let real = real_carrier_file();
    let real_violations = carrier_module_shape_violations(&real);
    assert!(
        real_violations.is_empty(),
        "the real sealed carrier.rs must scan clean under the exact-shape allowlist; found: \
         {real_violations:?}"
    );

    /// Apply `mutate` to a fresh real carrier.rs AST and report whether the scan
    /// now rejects it.
    fn rejects_mutation(mutate: impl FnOnce(&mut syn::File)) -> bool {
        let mut f = real_carrier_file();
        mutate(&mut f);
        !carrier_module_shape_violations(&f).is_empty()
    }

    // Bypass 1 (POINT 4) — a NON-CARRIER helper struct exposing `type_args`.
    // The prior kind-allowlist accepted any struct (merely derive-checking it).
    assert!(
        rejects_mutation(|f| {
            f.items
                .push(parse_item("struct ArgsHead { type_args: Arc<[SemanticNodeId]> }"));
        }),
        "DISCRIMINATION (POINT 4): a non-carrier helper `struct ArgsHead {{ type_args: … }}` must be \
         REJECTED — only the three carrier structs may exist."
    );

    // Bypass 2 (POINT 5) — a RENAMED import `use super::SemanticNodeId as NodeId;`.
    assert!(
        rejects_mutation(|f| {
            f.items
                .push(parse_item("use super::SemanticNodeId as NodeId;"));
        }),
        "DISCRIMINATION (POINT 5): a renamed import `use super::SemanticNodeId as NodeId;` must be \
         REJECTED — no `use … as …`, no imports beyond the exact sanctioned set."
    );

    // Bypass 3 (POINT 3) — an off-spec head alias `type BareRefHead<'a> = &'a [NodeId];`.
    // Note `NodeId` (not `SemanticNodeId`) would dodge a SemanticNodeId-mention
    // check; the exact-RHS pin rejects it on shape.
    assert!(
        rejects_mutation(|f| {
            let alias = find_head_alias_mut(f, "BareRefHead");
            alias.ty = Box::new(syn::parse_str::<syn::Type>("&'a [NodeId]").unwrap());
        }),
        "DISCRIMINATION (POINT 3): a head alias whose RHS is `&'a [NodeId]` (off the sanctioned \
         definition) must be REJECTED on exact shape."
    );

    // Bypass 7 (POINT 9) — an EXTRA method on `impl SemanticNodeData`.
    assert!(
        rejects_mutation(|f| {
            let im = find_impl_mut(f, "SemanticNodeData");
            im.items.push(syn::ImplItem::Fn(parse_impl_method(
                "pub(crate) fn extra(&self) -> u8 { 0 }",
            )));
        }),
        "DISCRIMINATION (POINT 9): an extra method on `impl SemanticNodeData` must be REJECTED — \
         the accessor block may carry ONLY the eight sanctioned accessors."
    );

    // Bypass 8 (POINT 8) — an EXTRA private helper on a carrier impl.
    assert!(
        rejects_mutation(|f| {
            let im = find_impl_mut(f, "BareRefCarrier");
            im.items
                .push(syn::ImplItem::Fn(parse_impl_method("fn helper(&self) -> u8 { 0 }")));
        }),
        "DISCRIMINATION (POINT 8): an extra private helper on a carrier impl must be REJECTED — the \
         carrier's inherent impl may carry ONLY its exact method set."
    );

    // Bypass 9a (POINT 6) — an EXTRA derive on a carrier struct.
    assert!(
        rejects_mutation(|f| {
            find_struct_mut(f, "TypeOfCarrier")
                .attrs
                .push(parse_attr("#[derive(Default)]"));
        }),
        "DISCRIMINATION (POINT 6): an extra derive (`Default`) on a carrier struct must be REJECTED \
         — carriers derive EXACTLY the five built-ins."
    );

    // Bypass 9b (POINT 6) — a `#[cfg_attr]` on a carrier struct.
    assert!(
        rejects_mutation(|f| {
            find_struct_mut(f, "TypeOfCarrier")
                .attrs
                .push(parse_attr("#[cfg_attr(feature = \"x\", derive(Clone))]"));
        }),
        "DISCRIMINATION (POINT 6): a `#[cfg_attr]` on a carrier struct must be REJECTED — only \
         `#[derive(...)]` + doc are permitted."
    );

    // Bypass 9c (POINT 6) — a qualified derive path `serde::Serialize`.
    assert!(
        rejects_mutation(|f| {
            let s = find_struct_mut(f, "TypeOfCarrier");
            s.attrs.clear();
            s.attrs.push(parse_attr(
                "#[derive(Clone, PartialEq, Eq, Hash, Debug, serde::Serialize)]",
            ));
        }),
        "DISCRIMINATION (POINT 6): a qualified / custom derive (`serde::Serialize`) must be \
         REJECTED."
    );

    // Bypass 10 (POINT 4) — a nested `mod`.
    assert!(
        rejects_mutation(|f| {
            f.items.push(parse_item("mod leak { }"));
        }),
        "DISCRIMINATION (POINT 4): a nested `mod` must be REJECTED — a submodule could synthesise a \
         leaking surface this file-scoped guard does not police."
    );

    // Bypass 11a (POINT 11) — a MACRO in a sanctioned body (`carrier_type_args`).
    // Macros are banned in ALL bodies, even the ones allowed to read raw args.
    assert!(
        rejects_mutation(|f| {
            let im = find_impl_mut(f, "SemanticNodeData");
            let m = find_method_mut(im, "carrier_type_args");
            m.block.stmts.insert(0, parse_stmt("let _ = vec![0u8];"));
        }),
        "DISCRIMINATION (POINT 11): a macro invocation (`vec![]`) inside `carrier_type_args` must \
         be REJECTED — no body in carrier.rs may invoke a macro."
    );

    // Bypass 11b (POINT 11) — a raw `.type_args` READ in a NON-sanctioned body.
    // `value_root` is not in the descent/rebuild set, so it may not touch the
    // raw args.
    assert!(
        rejects_mutation(|f| {
            let im = find_impl_mut(f, "TypeOfCarrier");
            let m = find_method_mut(im, "value_root");
            m.block
                .stmts
                .insert(0, parse_stmt("let _leak = &self.type_args;"));
        }),
        "DISCRIMINATION (POINT 11): a raw `self.type_args` read in `value_root` (a non-sanctioned \
         body) must be REJECTED — only the descent/rebuild bodies may touch a carrier's raw args."
    );

    // Bypass — a SIGNATURE drift on a sanctioned accessor (return the owned args
    // instead of borrowed). Exact-signature pinning rejects it.
    assert!(
        rejects_mutation(|f| {
            let im = find_impl_mut(f, "SemanticNodeData");
            let m = find_method_mut(im, "carrier_type_args");
            m.sig = parse_impl_method("fn carrier_type_args(&self) -> Arc<[SemanticNodeId]> { todo!() }")
                .sig;
        }),
        "DISCRIMINATION (POINT 9): a signature drift on `carrier_type_args` must be REJECTED — each \
         accessor is pinned to its EXACT signature."
    );

    // Bypass — a VISIBILITY drift on a sanctioned accessor (`carrier_type_args`
    // made `pub`). Exact-visibility pinning rejects it.
    assert!(
        rejects_mutation(|f| {
            let im = find_impl_mut(f, "SemanticNodeData");
            find_method_mut(im, "carrier_type_args").vis =
                syn::parse_str::<syn::Visibility>("pub").unwrap();
        }),
        "DISCRIMINATION (POINT 9): a visibility drift (`pub` instead of `pub(crate)`) on \
         `carrier_type_args` must be REJECTED."
    );

    // Bypass — a NON-PRIVATE carrier method (`pub(super)` re-exposes it to the
    // parent). Carrier members must be strictly non-escaping.
    assert!(
        rejects_mutation(|f| {
            let im = find_impl_mut(f, "BareRefCarrier");
            find_method_mut(im, "arg_nodes").vis =
                syn::parse_str::<syn::Visibility>("pub(super)").unwrap();
        }),
        "DISCRIMINATION (POINT 8): a `pub(super)` carrier method must be REJECTED — every carrier \
         payload method must be strictly PRIVATE."
    );

    // Bypass — a manual trait impl on a carrier (the trait-surface leak shape) and on a
    // REFERENCE carrier self-ty (the reference-self-ty unwrap shape).
    assert!(
        rejects_mutation(|f| {
            f.items.push(parse_item(
                "impl AsRef<[SemanticNodeId]> for BareRefCarrier { fn as_ref(&self) -> &[SemanticNodeId] { todo!() } }",
            ));
        }),
        "DISCRIMINATION: a manual `impl AsRef<[SemanticNodeId]> for BareRefCarrier` must be REJECTED."
    );
    assert!(
        rejects_mutation(|f| {
            f.items.push(parse_item(
                "impl AsRef<[SemanticNodeId]> for &BareRefCarrier { fn as_ref(&self) -> &[SemanticNodeId] { todo!() } }",
            ));
        }),
        "DISCRIMINATION: a manual trait impl on a REFERENCE carrier self-ty \
         `&BareRefCarrier` must be REJECTED."
    );

    // POINT 3-completeness — a MISSING required item is a deviation too: drop an
    // accessor and the scan must reject.
    assert!(
        rejects_mutation(|f| {
            let im = find_impl_mut(f, "SemanticNodeData");
            im.items
                .retain(|ii| !matches!(ii, syn::ImplItem::Fn(g) if g.sig.ident == "typeof_head"));
        }),
        "DISCRIMINATION: a MISSING sanctioned accessor (`typeof_head` removed) must be REJECTED — \
         the allowlist requires every described item to be present."
    );
}

/// Find the named head-view `type` alias in a parsed file, mutably (self-test
/// helper).
fn find_head_alias_mut<'a>(file: &'a mut syn::File, name: &str) -> &'a mut syn::ItemType {
    file.items
        .iter_mut()
        .find_map(|it| match it {
            syn::Item::Type(t) if t.ident == name => Some(t),
            _ => None,
        })
        .unwrap_or_else(|| panic!("head alias `{name}` not found"))
}

/// SELF-TEST for the module-decl rule ([`carrier_module_decl_violations`],
/// POINT 1). The real `semantic_query.rs` declares `pub mod carrier;` clean;
/// an inline body, a `#[path]` redirect, a `#[cfg]` gate, or a missing decl is a
/// deviation. Reverting the rule (e.g. dropping the `content.is_some()` /
/// attr checks) makes the matching assertion FAIL.
#[test]
fn carrier_module_decl_discriminates() {
    // The real declaration is clean.
    let real = parse(SEMANTIC_QUERY_RS);
    assert!(
        carrier_module_decl_violations(&real).is_empty(),
        "the real `pub mod carrier;` decl must be clean; found: {:?}",
        carrier_module_decl_violations(&real)
    );

    fn rejects(src: &str) -> bool {
        let file: syn::File = syn::parse_str(src).expect("synthetic semantic_query.rs must parse");
        !carrier_module_decl_violations(&file).is_empty()
    }

    assert!(
        !rejects("pub mod carrier;"),
        "an unadorned `pub mod carrier;` must be ACCEPTED"
    );
    assert!(
        !rejects("mod carrier;"),
        "an unadorned `mod carrier;` (private) must be ACCEPTED"
    );
    assert!(
        rejects("#[path = \"elsewhere.rs\"] pub mod carrier;"),
        "DISCRIMINATION (POINT 1): a `#[path]` redirect on `mod carrier` must be REJECTED — it \
         could point the scan at the wrong source."
    );
    assert!(
        rejects("#[cfg(test)] pub mod carrier;"),
        "DISCRIMINATION (POINT 1): a `#[cfg]` gate on `mod carrier` must be REJECTED — it could \
         conditionally swap the module."
    );
    assert!(
        rejects("pub mod carrier { pub fn leak() {} }"),
        "DISCRIMINATION (POINT 1): an inline `mod carrier {{ … }}` body must be REJECTED — it moves \
         the carrier surface out of the policed carrier.rs file."
    );
    assert!(
        rejects("pub mod something_else;"),
        "DISCRIMINATION (POINT 1): a missing `mod carrier;` decl must be REJECTED."
    );
}

/// SELF-TEST for the manual-trait-on-carrier predicate ([`impl_is_manual_trait_on_carrier`]),
/// reused by the exact-shape scan's trait-impl branch. A manual
/// `impl AsRef<[SemanticNodeId]> for BareRefCarrier` (a crate-wide trait method
/// leaking the raw args) is detected; a `#[derive(...)]`-only carrier emits NO
/// impl block, so it is not flagged. Reverting the predicate to always-false
/// makes the REJECT assertions FAIL.
#[test]
fn carrier_trait_impl_detector_discriminates() {
    // Attack: a manual `impl AsRef<[SemanticNodeId]> for BareRefCarrier` — a
    // crate-wide trait method that hands out the raw args. REJECTED.
    let leak = parse_impl_block(
        "impl AsRef<[SemanticNodeId]> for BareRefCarrier { \
            fn as_ref(&self) -> &[SemanticNodeId] { &self.type_args } \
         }",
    );
    assert!(
        impl_is_manual_trait_on_carrier(&leak),
        "DISCRIMINATION: a manual `impl AsRef<[SemanticNodeId]> for BareRefCarrier` is a forbidden \
         trait impl on a carrier struct — it leaks the raw args crate-wide."
    );

    // A manual `Deref` impl is likewise a trait impl on a carrier. REJECTED.
    let deref = parse_impl_block(
        "impl Deref for TypeOfCarrier { \
            type Target = [SemanticNodeId]; \
            fn deref(&self) -> &Self::Target { &self.type_args } \
         }",
    );
    assert!(
        impl_is_manual_trait_on_carrier(&deref),
        "DISCRIMINATION: a manual `impl Deref for TypeOfCarrier` is a forbidden trait impl."
    );

    // A manual trait impl on a REFERENCE carrier self-ty
    // (`impl AsRef<…> for &BareRefCarrier`) must be detected. The self-ty is a
    // `syn::Type::Reference`, which a direct `type_path_last_ident`-only check
    // returned `None` for and MISSED — `unwrap_type_layers` peels the
    // reference/paren/group wrappers before the carrier-name check. REJECTED.
    let ref_self = parse_impl_block(
        "impl AsRef<[SemanticNodeId]> for &BareRefCarrier { \
            fn as_ref(&self) -> &[SemanticNodeId] { todo!() } \
         }",
    );
    assert!(
        impl_is_manual_trait_on_carrier(&ref_self),
        "DISCRIMINATION: a manual trait impl on a REFERENCE carrier self-ty `&BareRefCarrier` must \
         be detected — the carrier-name check unwraps reference/paren/group self-types first; a \
         direct `type_path_last_ident`-only form saw a `<non-path-type>` and missed it."
    );

    // A `Paren`-wrapped carrier self-ty is likewise unwrapped. REJECTED.
    let paren_self = parse_impl_block(
        "impl Clone for (ImportTypeCarrier) { fn clone(&self) -> Self { todo!() } }",
    );
    assert!(
        impl_is_manual_trait_on_carrier(&paren_self),
        "DISCRIMINATION: a parenthesised carrier self-ty `(ImportTypeCarrier)` must be detected."
    );

    // An INHERENT impl on a carrier (no `trait_`) is NOT a manual trait impl —
    // it returns false here (its methods are policed by the carrier-impl checks
    // instead).
    let inherent =
        parse_impl_block("impl BareRefCarrier { fn name(&self) -> &Arc<str> { &self.name } }");
    assert!(
        !impl_is_manual_trait_on_carrier(&inherent),
        "an inherent `impl BareRefCarrier {{ … }}` is not a manual trait impl"
    );

    // A trait impl on a NON-carrier struct is irrelevant to carrier.rs's rule.
    let other =
        parse_impl_block("impl Clone for SomethingElse { fn clone(&self) -> Self { todo!() } }");
    assert!(
        !impl_is_manual_trait_on_carrier(&other),
        "a trait impl on a non-carrier struct is not policed by this rule"
    );

    // A `#[derive(...)]`-only carrier struct emits NO `syn::Item::Impl` (derives
    // are attributes), so the trait-impl rule finds nothing to flag — ACCEPTED.
    let derive_only: syn::File = syn::parse_str(
        "#[derive(Clone, PartialEq, Eq, Hash, Debug)] \
         pub struct BareRefCarrier { name: Arc<str>, type_args: Arc<[SemanticNodeId]> }",
    )
    .expect("parse derive-only struct");
    let flagged = derive_only
        .items
        .iter()
        .any(|it| matches!(it, syn::Item::Impl(im) if impl_is_manual_trait_on_carrier(im)));
    assert!(
        !flagged,
        "a `#[derive(...)]`-only carrier struct has no manual trait impl to flag — derives are \
         attributes, not impl blocks"
    );
}

/// SELF-TEST (catch-all detection). Weak-form attacks the OLD `Pat::Wild`-only
/// check missed: a bare-binding catch-all `other => …` (a `Pat::Ident`); the
/// same hidden inside a `|`-pattern (`Self::X(_) | other => …`); a parenthesised
/// `(_) =>`; a reference `&_ =>`; and an aliasing `x @ _ =>`. Reverting
/// `arm_is_catchall` to only flag `Pat::Wild` (or dropping the Paren / Reference
/// / Ident-subpat unwrap) makes the matching assertions below FAIL.
#[test]
fn accessor_catchall_detector_discriminates() {
    let arms = parse_match_arms(
        "match self { \
            _ => 0, \
            other => 1, \
            Self::X(_) | other => 2, \
            (_) => 3, \
            &_ => 4, \
            x @ _ => 5, \
            Self::TypeOf(c) => 6, \
            SemanticNodeData::BareRef(_) => 7, \
            Self::TypeOf(_) | Self::BareRef(_) => 8 \
         }",
    );

    // Catch-alls the strengthened detector MUST flag:
    assert!(arm_is_catchall(&arms[0].pat), "`_` is a wildcard catch-all");
    assert!(
        arm_is_catchall(&arms[1].pat),
        "DISCRIMINATION: a bare-binding `other =>` is a catch-all (a `Pat::Ident`, which the \
         `Pat::Wild`-only weak form would miss)."
    );
    assert!(
        arm_is_catchall(&arms[2].pat),
        "DISCRIMINATION: a `|`-pattern with a bare-binding alternative (`Self::X(_) | other`) is a \
         catch-all."
    );
    assert!(
        arm_is_catchall(&arms[3].pat),
        "DISCRIMINATION: a parenthesised `(_) =>` is a catch-all (unwrap `Pat::Paren`)."
    );
    assert!(
        arm_is_catchall(&arms[4].pat),
        "DISCRIMINATION: a reference `&_ =>` is a catch-all (unwrap `Pat::Reference`)."
    );
    assert!(
        arm_is_catchall(&arms[5].pat),
        "DISCRIMINATION: an aliasing `x @ _ =>` is a catch-all (recurse into the `Pat::Ident` \
         subpattern)."
    );

    // Real variant patterns the detector must NOT flag:
    assert!(
        !arm_is_catchall(&arms[6].pat),
        "a qualified tuple-struct variant `Self::TypeOf(c)` is not a catch-all"
    );
    assert!(
        !arm_is_catchall(&arms[7].pat),
        "a qualified tuple-struct variant with an inner wildcard `BareRef(_)` is not a catch-all"
    );
    assert!(
        !arm_is_catchall(&arms[8].pat),
        "an Or of variant patterns is not a catch-all"
    );
}

/// SELF-TEST (match-self targeting). The strengthened locator [`match_self_exprs`]
/// targets the `match self` SPECIFICALLY, not the first `match` in the block. A
/// body with a harmless leading `match other { _ => 0 }` (a catch-all, but NOT
/// on `self`) followed by the real `match self { Self::A(_) => &[], _ => &[] }`
/// would have let the OLD [`find_first_match`] locator inspect `match other`
/// (which has no variant arms to flag) and pass, never seeing the `_` in
/// `match self`. Reverting `match_self_exprs` to first-match semantics (or
/// `expr_is_self` to always-true) makes the targeting assertion FAIL.
#[test]
fn accessor_match_self_targeting_discriminates() {
    let body = parse_impl_method(
        "fn carrier_type_args(&self) -> &[u8] { \
            match other { _ => 0 }; \
            match self { Self::A(_) => &[], _ => &[] } \
         }",
    );

    // STRENGTHENED locator: finds the `match self` SPECIFICALLY (exactly one),
    // skipping the leading `match other`.
    let selves = match_self_exprs(&body.block);
    assert_eq!(
        selves.len(),
        1,
        "DISCRIMINATION: the strengthened locator must find the `match self` even behind a leading \
         `match other` (and must NOT also collect the `match other`)."
    );
    let match_self = selves[0];
    assert!(
        expr_is_self(&match_self.expr),
        "the collected match's scrutinee is `self`"
    );

    // Its `_ =>` tail arm is the catch-all the strengthened guard now flags.
    assert!(
        match_self.arms.iter().any(|a| arm_is_catchall(&a.pat)),
        "DISCRIMINATION: the `_ =>` arm in `match self` must be flagged as a catch-all — the hole \
         the OLD first-match locator left open."
    );

    // Cross-check the OLD weak form's blind spot: the FIRST `match` in the block
    // is `match other` (NOT `match self`), so the old locator inspected the
    // wrong match and would have passed.
    let first = find_first_match(&body.block).expect("a leading match exists");
    assert!(
        !expr_is_self(&first.expr),
        "the FIRST match in the block is `match other`, NOT `match self` — exactly why the OLD \
         `find_first_match`-based guard missed the catch-all in `match self`."
    );
}
