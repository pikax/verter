//! Architecture guard: EVERY authored template-expression value in the Svelte
//! client backend routes through the SOLE preparation entry
//! (`SupportedClientIr::prepare_template_value(AuthoredValueInput, AuthoredValueSurface)`
//! in `client_legacy_value.rs`) — the surface-keyed `policy()` is the ONE
//! wrap-vs-raw decision point; emitters serialize prepared carriers and never
//! decide whether legacy wrapping applies.
//!
//! HONEST SCOPE of this guard (what it proves, and what it does not):
//! For the currently inventoried T2 authored-value surfaces, planning routes values through `prepare_template_value`, whose sealed carrier and exhaustive policy prevent independent construction or selection of the legacy wrap.
//! This does not make authored-to-emission routing impossible by type across the whole client backend: several narrow-plan fields retain serialized authored expressions or synthesized topology containing them as `String`.
//! The structural inventories are fail-closed for known rewrite and surface call sites, but a new planner can evade them by copying source or rewritten text into a raw-string plan field.
//! They are therefore transitional structural enforcement, not proof of a terminal capability boundary.
//! The terminal type/capability boundary is a backend-wide concern tracked as
//! D-61 in `docs/arch/svelte-native-compiler-plan.md`.
//!
//! RAIL 1 is the TYPE BOUNDARY, enforced by rustc at compile time:
//! `PreparedTemplateValue` has private fields and `PreparedExpression`
//! (the `LegacySequence` wrap carrier) is PRIVATE to the owner module, so wrap
//! construction is owner-only; `SynthesizedTemplateValue` is sealed in its
//! dedicated module behind a RAW-only accessor API with a closed TYPED
//! constructor vocabulary; the preparation input is the closed
//! `AuthoredValueInput` vocabulary; and the `policy()` match is exhaustive
//! over `AuthoredValueSurface` (a new variant fails compilation until
//! classified). This guard RE-VERIFIES those boundaries structurally — by
//! parsing each runtime module with `syn` and inspecting the AST — so a
//! REGRESSION of the boundary (re-publicizing the wrap carrier, adding a
//! wildcard arm, opening a second `prepare_*` entry, a new `AuthoredExpr`
//! consumer, re-typing one of the EXPLICITLY-PINNED carrier fields to
//! `String`) fails in-tree. The field pins cover exactly the inventoried
//! fields — a NEW raw-string plan field is the D-61 class named in the
//! honest-scope paragraph above (transitional structural coverage, not
//! proven impossible here). AST analysis is whitespace-proof and ignores
//! comments/strings by construction.
//!
//! RAIL 2 is the expression-serialization CALL-SITE INVENTORY: syn-AST
//! discovery of every call of the general rewrite family (`self.rewrite`,
//! `rewrite_source`, the statement/rune/plain-JS lanes, the value printer,
//! and the underlying `expr_rewrite` entries) across the runtime backend,
//! each proven to be an explicitly-classified NON-authored-value role — a
//! KNOWN-family `ExprId -> String` call on an authored value fails the gate
//! until consciously classified (fail-closed for the inventoried family; see
//! the honest-scope paragraph above for what it cannot see).
//!
//! RAIL 3 is the AUTHORED-POSITION → SURFACE BINDING INVENTORY: every call
//! of a surface-accepting fn (the preparation entry, the policy table, and
//! every forwarding helper — discovered structurally from
//! `AuthoredValueSurface`-typed parameters) is pinned to its REQUIRED
//! surface. A new authored position that selects an existing Raw-classified
//! surface for a should-wrap value routes THROUGH the entry yet still emits
//! raw — the serialization inventory cannot see it; this binding table can.
//! The scan universe for all three rails is the COMPLETE RECURSIVE production
//! module graph under `src/svelte/runtime/` (subdirectories included);
//! test-only exclusion is proven per module from the `#[cfg(test)]` module
//! graph, never assumed from a filename.
//!
//! SECONDARY (tripwire only): a comment-stripped substring scan for the
//! RETIRED entry-point names and for legacy-wrap SYNTAX construction outside
//! the owner. These detect reintroduction by exact spelling only — they
//! establish no completeness and are NOT the enforcement mechanism.
//!
//! What the structural rails do NOT cover: an emitter can still `format!`
//! arbitrary output bytes, and raw-string plan fields can carry authored
//! text past the inventories (the honest-scope paragraph above). Those
//! residual classes are covered by the secondary wrap-syntax tripwire plus
//! the behavioral oracle goldens and the conformance value-wrap cells — and
//! terminally by the D-61 capability boundary, not by this guard.
//!
//! Registered in `CRITICAL_RULE_GUARDS`
//! (`crates/verter_session/tests/g_misc0/critical_rules_have_guards.rs`).

use super::svelte_guard_support;

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use quote::ToTokens;
use svelte_guard_support::strip_rust_comments;

fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read_runtime_source(name: &str) -> String {
    let path = crate_root().join("src/svelte/runtime").join(name);
    fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read runtime file {}: {e}", path.display()))
}

/// Parse one runtime module into its `syn` AST (comments and formatting are
/// discarded by real parsing — the structural scans below cannot be defeated
/// by whitespace, renamed-lookalike strings, or comment mentions).
fn parse_runtime_module(name: &str) -> syn::File {
    syn::parse_file(&read_runtime_source(name))
        .unwrap_or_else(|e| panic!("syn-parse runtime file {name}: {e}"))
}

/// The owner module of the preparation entry + policy + wrap carrier.
const OWNER: &str = "client_legacy_value.rs";

/// The module DEFINING the low-level `ExprId -> String` value printer
/// (`rewrite_value_preserving_source`) the owner consumes — the definition
/// site is allowed to name it; every other module is not.
const PRINTER_DEFINER: &str = "client_plan_rewrite.rs";

/// The authored-value PLANNER modules — the closed inventory of production
/// modules allowed to consume `AuthoredExpr` (construct preparation inputs).
/// A module outside this list must not prepare authored values; adding one is
/// a conscious vocabulary change, made here.
const AUTHORED_VALUE_PLANNERS: &[&str] = &[
    "client_block_plan.rs",
    "client_component_plan.rs",
    "client_plan.rs",
    "client_plan_attr_value.rs",
    "client_plan_bind.rs",
    "client_plan_element_ops.rs",
    "client_plan_spread_html.rs",
    "client_slot_plan.rs",
    "client_svelte_boundary.rs",
    "client_svelte_element.rs",
    "client_svelte_head.rs",
];

// ─────────────────────────── syn AST helpers ────────────────────────────────

/// One out-of-line `mod` declaration discovered in a parsed file: the
/// candidate file paths it resolves to (RELATIVE to the declaring file's
/// directory) and whether the declaration chain is `#[cfg(test)]`-gated.
struct OutOfLineModDecl {
    /// Candidate file paths relative to the declaring file's directory (a
    /// `#[path]` declaration yields one exact candidate; a plain `mod x;`
    /// yields the `x.rs` / `x/mod.rs` pair under the style-correct base).
    candidates: Vec<PathBuf>,
    /// Whether the declaration (or a transitive inline-mod ancestor) carries
    /// `#[cfg(test)]` — the SOLE test-only signal; filenames never classify.
    test_only: bool,
}

/// The `#[path = "…"]` attribute payload of a `mod` item, if present.
fn mod_path_attr(attrs: &[syn::Attribute]) -> Option<String> {
    attrs.iter().find_map(|a| {
        if !a.path().is_ident("path") {
            return None;
        }
        let syn::Meta::NameValue(nv) = &a.meta else {
            return None;
        };
        let syn::Expr::Lit(lit) = &nv.value else {
            return None;
        };
        let syn::Lit::Str(s) = &lit.lit else {
            return None;
        };
        Some(s.value())
    })
}

/// Collect every out-of-line `mod` declaration of `items`, resolving each to
/// its candidate file paths per the Rust module-resolution rules:
///
/// - top-level `#[path = "P"] mod x;` → `P` (relative to the file's dir, for
///   BOTH mod-rs and non-mod-rs declaring files);
/// - top-level `mod x;` → `x.rs` / `x/mod.rs` for a mod-rs file,
///   `<stem>/x.rs` / `<stem>/x/mod.rs` for a non-mod-rs file;
/// - inside inline `mod a { … }` blocks the base extends with the inline
///   names (and the `#[path]` base matches the default base).
///
/// `inherited_test` carries a `#[cfg(test)]` gate on an enclosing inline mod
/// down to every nested declaration.
fn collect_out_of_line_mods(
    items: &[syn::Item],
    inherited_test: bool,
    at_top_level: bool,
    default_base: &std::path::Path,
    out: &mut Vec<OutOfLineModDecl>,
) {
    for item in items {
        let syn::Item::Mod(m) = item else {
            continue;
        };
        let test_only = inherited_test || is_cfg_test(&m.attrs);
        match &m.content {
            None => {
                let candidates = match mod_path_attr(&m.attrs) {
                    Some(p) if at_top_level => vec![PathBuf::from(p)],
                    Some(p) => vec![default_base.join(p)],
                    None => vec![
                        default_base.join(format!("{}.rs", m.ident)),
                        default_base.join(m.ident.to_string()).join("mod.rs"),
                    ],
                };
                out.push(OutOfLineModDecl {
                    candidates,
                    test_only,
                });
            }
            Some((_, inner)) => {
                let inner_base = default_base.join(m.ident.to_string());
                collect_out_of_line_mods(inner, test_only, false, &inner_base, out);
            }
        }
    }
}

/// Walk the ACTUAL module graph from a root `mod.rs`, resolving every
/// out-of-line `mod` declaration to its file, and classify each reached
/// file's test-only status: a file is test-only iff its `mod` declaration —
/// or a transitive ancestor declaration — carries `#[cfg(test)]`. Root-relative
/// `/`-separated names map to the classification; a file declared both ways
/// classifies PRODUCTION (fail-closed: it joins the scan rails).
fn classify_module_graph(root: &std::path::Path) -> std::collections::BTreeMap<String, bool> {
    let mut classified = std::collections::BTreeMap::new();
    let mut visited = BTreeSet::new();
    // (root-relative file, is mod-rs style, declaration-chain test status)
    let mut queue: Vec<(PathBuf, bool, bool)> = vec![(PathBuf::from("mod.rs"), true, false)];
    while let Some((rel, is_mod_rs, test_only)) = queue.pop() {
        if !visited.insert((rel.clone(), test_only)) {
            continue;
        }
        let abs = root.join(&rel);
        let source = fs::read_to_string(&abs)
            .unwrap_or_else(|e| panic!("read module-graph file {}: {e}", abs.display()));
        let file = syn::parse_file(&source)
            .unwrap_or_else(|e| panic!("syn-parse module-graph file {}: {e}", abs.display()));
        let dir_rel = rel.parent().map(PathBuf::from).unwrap_or_default();
        let default_base = if is_mod_rs {
            PathBuf::new()
        } else {
            PathBuf::from(rel.file_stem().expect("module file stem"))
        };
        let mut decls = Vec::new();
        collect_out_of_line_mods(&file.items, test_only, true, &default_base, &mut decls);
        for decl in decls {
            let Some(resolved) = decl
                .candidates
                .iter()
                .map(|c| dir_rel.join(c))
                .find(|c| root.join(c).is_file())
            else {
                // No candidate file under the root (an out-of-tree `#[path]`
                // target): nothing to classify — an on-disk file it does not
                // reach stays in the scan universe by default (fail-closed).
                continue;
            };
            let name = resolved
                .components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            let entry = classified.entry(name).or_insert(decl.test_only);
            // A production declaration always wins over a test-only one.
            *entry = *entry && decl.test_only;
            let child_is_mod_rs = resolved.file_name().map(|f| f == "mod.rs").unwrap_or(false);
            queue.push((resolved, child_is_mod_rs, decl.test_only));
        }
    }
    classified
}

/// The test-only module set of the runtime backend, proven from the ACTUAL
/// `#[cfg(test)]` module graph — NEVER from filename conventions. A module
/// named `*_tests.rs` whose `mod` declaration carries no `#[cfg(test)]` is
/// unconditionally-compiled PRODUCTION code and is NOT in this set, so it
/// joins every scan rail below.
fn test_only_runtime_modules() -> BTreeSet<String> {
    classify_module_graph(&crate_root().join("src/svelte/runtime"))
        .into_iter()
        .filter_map(|(name, test_only)| test_only.then_some(name))
        .collect()
}

/// Every `.rs` module of the client runtime backend (the scan universe) —
/// the COMPLETE RECURSIVE production module graph under
/// `src/svelte/runtime/`, subdirectories included (`css/`, `expr_rewrite/`,
/// and any future submodule dir), so a planner placed under a nested
/// submodule cannot evade the consumer/serialization/position inventories.
/// Names are root-relative with `/` separators. Excludes exactly the modules
/// PROVEN `#[cfg(test)]`-declared in the actual module graph (unit-test
/// siblings exercise carriers through the public pipeline by design); the
/// classification is by declaration, never by filename, so an
/// unconditionally-compiled module named `*_tests.rs` stays in EVERY rail. A
/// file with no reachable `mod` declaration also stays in the universe
/// (fail-closed: the scans see a planted file before it even compiles).
fn runtime_modules() -> Vec<String> {
    let test_only = test_only_runtime_modules();
    let root = crate_root().join("src/svelte/runtime");
    let mut names = Vec::new();
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).expect("runtime dir") {
            let path = entry.expect("dir entry").path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().map(|x| x != "rs").unwrap_or(true) {
                continue;
            }
            let name = path
                .strip_prefix(&root)
                .expect("runtime file under runtime root")
                .components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            if !test_only.contains(&name) {
                names.push(name);
            }
        }
    }
    names.sort_unstable();
    names
}

/// Collect every `Ident` token in a token stream, recursively (groups are
/// walked; literals and punctuation are skipped — a string literal containing
/// a banned name is NOT an ident).
fn collect_idents(ts: proc_macro2::TokenStream, out: &mut Vec<String>) {
    for tree in ts {
        match tree {
            proc_macro2::TokenTree::Group(g) => collect_idents(g.stream(), out),
            proc_macro2::TokenTree::Ident(i) => out.push(i.to_string()),
            proc_macro2::TokenTree::Punct(_) | proc_macro2::TokenTree::Literal(_) => {}
        }
    }
}

/// The full ident inventory of a parsed file.
fn file_idents(file: &syn::File) -> Vec<String> {
    let mut out = Vec::new();
    collect_idents(file.to_token_stream(), &mut out);
    out
}

fn has_ident(file: &syn::File, name: &str) -> bool {
    file_idents(file).iter().any(|i| i == name)
}

/// Find an enum item by name.
fn find_enum<'f>(file: &'f syn::File, name: &str) -> Option<&'f syn::ItemEnum> {
    file.items.iter().find_map(|item| match item {
        syn::Item::Enum(e) if e.ident == name => Some(e),
        _ => None,
    })
}

/// Find a struct item by name.
fn find_struct<'f>(file: &'f syn::File, name: &str) -> Option<&'f syn::ItemStruct> {
    file.items.iter().find_map(|item| match item {
        syn::Item::Struct(s) if s.ident == name => Some(s),
        _ => None,
    })
}

/// Every inherent-impl method of the file as `(name, is_pub_super)`.
fn impl_methods(file: &syn::File) -> Vec<(String, bool)> {
    let mut out = Vec::new();
    for item in &file.items {
        if let syn::Item::Impl(imp) = item {
            for ii in &imp.items {
                if let syn::ImplItem::Fn(f) = ii {
                    let is_pub_super = !matches!(f.vis, syn::Visibility::Inherited);
                    out.push((f.sig.ident.to_string(), is_pub_super));
                }
            }
        }
    }
    out
}

/// The inherent-impl method names of ONE named type in the file (trait impls
/// — `Debug` / `Clone` derives expand elsewhere — do not contribute).
fn impl_method_names_of(file: &syn::File, type_name: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for item in &file.items {
        if let syn::Item::Impl(imp) = item {
            if imp.trait_.is_some() {
                continue;
            }
            let syn::Type::Path(p) = imp.self_ty.as_ref() else {
                continue;
            };
            if p.path.segments.last().map(|s| s.ident != type_name) != Some(false) {
                continue;
            }
            for ii in &imp.items {
                if let syn::ImplItem::Fn(f) = ii {
                    out.insert(f.sig.ident.to_string());
                }
            }
        }
    }
    out
}

/// Every `(container, field, type)` triple of the file's structs and enums —
/// tuple fields are named by index. The type is the token-rendered path with
/// spaces removed (`Option<PreparedIfCondition>`).
fn container_fields(file: &syn::File) -> Vec<(String, String, String)> {
    fn push_fields(container: &str, fields: &syn::Fields, out: &mut Vec<(String, String, String)>) {
        match fields {
            syn::Fields::Named(named) => {
                for f in &named.named {
                    out.push((
                        container.to_string(),
                        f.ident.as_ref().unwrap().to_string(),
                        f.ty.to_token_stream().to_string().replace(' ', ""),
                    ));
                }
            }
            syn::Fields::Unnamed(unnamed) => {
                for (i, f) in unnamed.unnamed.iter().enumerate() {
                    out.push((
                        container.to_string(),
                        i.to_string(),
                        f.ty.to_token_stream().to_string().replace(' ', ""),
                    ));
                }
            }
            syn::Fields::Unit => {}
        }
    }
    let mut out = Vec::new();
    for item in &file.items {
        match item {
            syn::Item::Struct(s) => push_fields(&s.ident.to_string(), &s.fields, &mut out),
            syn::Item::Enum(e) => {
                for v in &e.variants {
                    push_fields(&format!("{}::{}", e.ident, v.ident), &v.fields, &mut out);
                }
            }
            _ => {}
        }
    }
    out
}

/// The arms of the FIRST match expression inside a free fn named `fn_name`:
/// `(variant_names, saw_wildcard_or_non_path_pattern)`.
fn match_arm_variants(file: &syn::File, fn_name: &str) -> (Vec<String>, bool) {
    let f = file
        .items
        .iter()
        .find_map(|item| match item {
            syn::Item::Fn(f) if f.sig.ident == fn_name => Some(f),
            _ => None,
        })
        .unwrap_or_else(|| panic!("free fn {fn_name} present"));
    let m = f
        .block
        .stmts
        .iter()
        .find_map(|stmt| match stmt {
            syn::Stmt::Expr(syn::Expr::Match(m), _) => Some(m),
            _ => None,
        })
        .unwrap_or_else(|| panic!("{fn_name} body is a match expression"));
    let mut variants = Vec::new();
    let mut non_path = false;
    for arm in &m.arms {
        match &arm.pat {
            syn::Pat::Path(p) => {
                variants.push(p.path.segments.last().unwrap().ident.to_string());
            }
            // A wildcard, or-pattern, binding, rest, or any other pattern kind
            // would classify surfaces away from one-arm-per-variant.
            _ => non_path = true,
        }
    }
    (variants, non_path)
}

// ────────────────────── RAIL 1: type-boundary structure ─────────────────────

#[test]
fn policy_match_is_exhaustive_over_every_surface_without_wildcard() {
    // rustc already enforces exhaustiveness; this AST scan enforces the
    // STRONGER shape: one named arm per variant — no wildcard, no or-pattern,
    // no binding — so a future surface cannot land without its own conscious
    // wrap-vs-raw ruling.
    let owner = parse_runtime_module(OWNER);
    let surface = find_enum(&owner, "AuthoredValueSurface").expect("AuthoredValueSurface enum");
    let variants: BTreeSet<String> = surface
        .variants
        .iter()
        .map(|v| v.ident.to_string())
        .collect();
    assert!(
        variants.len() >= 30,
        "the surface vocabulary must stay non-vacuous (found {})",
        variants.len()
    );
    let (arm_variants, non_path) = match_arm_variants(&owner, "policy");
    assert!(
        !non_path,
        "policy() must carry only named per-variant arms (no wildcard / \
         or-pattern / binding arm)"
    );
    let arms: BTreeSet<String> = arm_variants.iter().cloned().collect();
    assert_eq!(
        arms.len(),
        arm_variants.len(),
        "policy() arms must be distinct (one arm per variant)"
    );
    assert_eq!(
        arms, variants,
        "policy() must classify exactly the AuthoredValueSurface variants"
    );
}

#[test]
fn wrap_construction_is_owner_only_by_visibility_and_reference() {
    // The wrap carrier (`PreparedExpression` / its `LegacySequence` variant)
    // is PRIVATE to the owner: rustc already rejects an out-of-module
    // constructor; this scan pins the visibility so a re-publicizing edit
    // fails, and verifies no other runtime module references the carrier or
    // the wrap core AT ALL.
    let owner = parse_runtime_module(OWNER);
    let prepared = find_enum(&owner, "PreparedExpression").expect("PreparedExpression enum");
    assert!(
        matches!(prepared.vis, syn::Visibility::Inherited),
        "PreparedExpression must stay PRIVATE to client_legacy_value.rs \
         (owner-only wrap construction)"
    );
    // The wrap core is a private method of the owner.
    let owner_methods = impl_methods(&owner);
    let wrap_core = owner_methods
        .iter()
        .find(|(name, _)| name == "legacy_wrap_rewritten")
        .expect("the wrap core lives in the owner");
    assert!(
        !wrap_core.1,
        "legacy_wrap_rewritten must stay a PRIVATE method of the owner"
    );
    // No other runtime module references the carrier or the wrap core.
    for module in runtime_modules() {
        if module == OWNER {
            continue;
        }
        let file = parse_runtime_module(&module);
        for banned in [
            "LegacySequence",
            "legacy_wrap_rewritten",
            "PreparedExpression",
        ] {
            assert!(
                !has_ident(&file, banned),
                "{module} must not reference the owner-private `{banned}`"
            );
        }
    }
}

/// The DEDICATED module sealing `SynthesizedTemplateValue`: struct + the
/// three typed constructors + the raw-only accessors and NOTHING else, so no
/// struct-literal or free-fn `String` construction is possible from any other
/// module (rustc enforces it via truly module-private fields).
const SYNTHESIZED_OWNER: &str = "synthesized_value.rs";

#[test]
fn synthesized_carrier_is_sealed_raw_only() {
    // `SynthesizedTemplateValue` is sealed in its DEDICATED module: private
    // fields (rustc blocks field construction/mutation outside
    // `synthesized_value.rs`) and a RAW-only accessor vocabulary — no method
    // yields a wrapped rendering, and the carrier has no wrap-typed state to
    // hold one.
    let owner = parse_runtime_module(SYNTHESIZED_OWNER);
    let synth =
        find_struct(&owner, "SynthesizedTemplateValue").expect("SynthesizedTemplateValue struct");
    for field in &synth.fields {
        assert!(
            matches!(field.vis, syn::Visibility::Inherited),
            "SynthesizedTemplateValue field `{}` must be private (sealed carrier)",
            field
                .ident
                .as_ref()
                .map(|i| i.to_string())
                .unwrap_or_default()
        );
    }
    // The sealing is the MODULE boundary: no other runtime module may define
    // a same-named struct (a move back into a shared module re-opens
    // same-module struct-literal construction to unrelated code).
    for module in runtime_modules() {
        if module == SYNTHESIZED_OWNER {
            continue;
        }
        assert!(
            find_struct(&parse_runtime_module(&module), "SynthesizedTemplateValue").is_none(),
            "{module} must not define SynthesizedTemplateValue (the sealed \
             carrier lives in its dedicated module {SYNTHESIZED_OWNER})"
        );
    }
}

/// The `syn::Item` variant name — for the fail-closed permit-list panic
/// message naming the prohibited kind.
fn syn_item_kind(item: &syn::Item) -> &'static str {
    match item {
        syn::Item::Const(_) => "const",
        syn::Item::Enum(_) => "enum",
        syn::Item::ExternCrate(_) => "extern crate",
        syn::Item::Fn(_) => "fn",
        syn::Item::ForeignMod(_) => "foreign mod",
        syn::Item::Impl(_) => "impl",
        syn::Item::Macro(_) => "macro",
        syn::Item::Mod(_) => "mod",
        syn::Item::Static(_) => "static",
        syn::Item::Struct(_) => "struct",
        syn::Item::Trait(_) => "trait",
        syn::Item::TraitAlias(_) => "trait alias",
        syn::Item::Type(_) => "type alias",
        syn::Item::Union(_) => "union",
        syn::Item::Use(_) => "use",
        _ => "unknown",
    }
}

/// The `syn::ImplItem` variant name — for the fail-closed associated-item
/// permit-list panic message naming the prohibited kind.
fn syn_impl_item_kind(item: &syn::ImplItem) -> &'static str {
    match item {
        syn::ImplItem::Const(_) => "const",
        syn::ImplItem::Fn(_) => "fn",
        syn::ImplItem::Type(_) => "type",
        syn::ImplItem::Macro(_) => "macro",
        syn::ImplItem::Verbatim(_) => "verbatim",
        _ => "unknown",
    }
}

/// The five sealed inherent-impl associated items of the carrier: the three
/// typed synthesis constructors + the two raw-only accessors. The impl may hold
/// ONLY these `fn`s.
const PERMITTED_SYNTHESIZED_IMPL_METHODS: [&str; 5] = [
    "clsx",
    "class_directives",
    "style_directives",
    "has_call",
    "raw_text",
];

/// FAIL-CLOSED permit-list over the sealed carrier's inherent-impl ASSOCIATED
/// ITEMS. Pinning the method NAMES (via [`impl_method_names_of`]) and the
/// fn-body construction sites is not enough: an associated `const` typed
/// `fn(String) -> Self` (`const ROGUE: fn(String) -> Self = |text| Self { text,
/// has_call: false }`), an associated `type`, or an item-`macro` all run inside
/// the fields' module-private scope and can struct-literal the sealed carrier
/// with arbitrary `text` while being invisible to both the `ImplItem::Fn`
/// method scan and the fn-body construction scan. So exhaustively inspect every
/// associated item: permit ONLY the five pinned `fn`s by exact name and PANIC on
/// any non-`fn` associated item (`const`/`type`/`macro`/`verbatim`/any future
/// `syn::ImplItem` variant, via the exhaustive-by-`_` kind helper) or any
/// unpinned method.
fn assert_synthesized_impl_items_sealed(imp: &syn::ItemImpl) {
    for ii in &imp.items {
        match ii {
            syn::ImplItem::Fn(f) => {
                let name = f.sig.ident.to_string();
                assert!(
                    PERMITTED_SYNTHESIZED_IMPL_METHODS.contains(&name.as_str()),
                    "{SYNTHESIZED_OWNER} inherent impl holds an unpinned method \
                     `{name}` — the sealed carrier permits ONLY the three typed \
                     constructors + the two raw-only accessors (a new one is a \
                     conscious vocabulary change made here)"
                );
            }
            other => panic!(
                "{SYNTHESIZED_OWNER} inherent impl holds a prohibited associated \
                 `{}` item — an associated const/type/macro shares the fields' \
                 module-private scope and can struct-literal the sealed carrier \
                 with arbitrary `text`; the impl permits ONLY the five pinned \
                 `fn` items",
                syn_impl_item_kind(other)
            ),
        }
    }
}

#[test]
fn synthesized_constructor_and_consumer_inventory_is_pinned() {
    // CONSTRUCTOR inventory: no free-form `new(String, bool)` — a text-typed
    // constructor lets any planner hand a synthesized carrier an authored raw
    // expression or a hand-built legacy sequence (semantic masquerading the
    // private fields alone cannot prevent). Construction is the closed
    // OWNER-SPECIFIC typed vocabulary — each constructor consumes PREPARED /
    // typed contributors and derives the rendered composite text ITSELF.
    let owner = parse_runtime_module(SYNTHESIZED_OWNER);
    let methods = impl_method_names_of(&owner, "SynthesizedTemplateValue");
    assert!(
        !methods.contains("new"),
        "SynthesizedTemplateValue must not expose a free-form `new` constructor"
    );
    let expected: BTreeSet<String> = [
        // The three typed synthesis constructors.
        "clsx",
        "class_directives",
        "style_directives",
        // The RAW-only accessor vocabulary.
        "has_call",
        "raw_text",
    ]
    .into_iter()
    .map(str::to_string)
    .collect();
    assert_eq!(
        methods, expected,
        "the SynthesizedTemplateValue method inventory is pinned: three typed \
         synthesis constructors + the raw-only accessors (adding one is a \
         conscious vocabulary change, reviewed here)"
    );
    // The dedicated module holds ONLY the sealed carrier — a CLOSED permit-list
    // over every `syn::Item` kind, fail-closed. Any other item shares the
    // fields' module-private scope (a `static LazyLock<SynthesizedTemplateValue>`,
    // a `const`, a `macro_rules!`, a nested `mod`, an `enum`/`trait`/type-alias
    // helper can all struct-literal the sealed carrier with arbitrary `text`),
    // so anything outside the permit-list PANICS instead of slipping through.
    for item in &owner.items {
        match item {
            // Imports cannot construct the carrier.
            syn::Item::Use(_) => {}
            syn::Item::Struct(s) => assert_eq!(
                s.ident, "SynthesizedTemplateValue",
                "{SYNTHESIZED_OWNER} must define only the sealed carrier struct"
            ),
            syn::Item::Impl(imp) => {
                assert!(
                    imp.trait_.is_none(),
                    "{SYNTHESIZED_OWNER} must hold no trait impl (a `From`/`Default` \
                     impl is a text-injection route into the sealed carrier)"
                );
                let self_ty = imp.self_ty.to_token_stream().to_string().replace(' ', "");
                assert_eq!(
                    self_ty, "SynthesizedTemplateValue",
                    "{SYNTHESIZED_OWNER} inherent impls must target only the sealed \
                     carrier"
                );
                // Exhaustively fail-closed over the impl's ASSOCIATED ITEMS: an
                // associated const/type/macro shares the fields' scope and can
                // struct-literal the sealed carrier, invisible to the method and
                // fn-body scans (the associated-const vector).
                assert_synthesized_impl_items_sealed(imp);
            }
            other => panic!(
                "{SYNTHESIZED_OWNER} holds a prohibited `{}` item — the sealed-carrier \
                 module permits ONLY `use` / the carrier struct / its inherent impl \
                 (same-module code can struct-literal the private fields)",
                syn_item_kind(other)
            ),
        }
    }
    // CONSUMER inventory: the modules referencing the carrier type (AST
    // idents — comments/strings never count).
    let mut consumers: BTreeSet<String> = BTreeSet::new();
    for module in runtime_modules() {
        if has_ident(&parse_runtime_module(&module), "SynthesizedTemplateValue") {
            consumers.insert(module);
        }
    }
    let expected_consumers: BTreeSet<String> = [
        SYNTHESIZED_OWNER,
        "client_plan_element_ops.rs",
        "client_plan_spread_html.rs",
        "client_plan_types.rs",
    ]
    .into_iter()
    .map(str::to_string)
    .collect();
    assert_eq!(
        consumers, expected_consumers,
        "the SynthesizedTemplateValue consumer set is the closed synthesis-owner \
         inventory (a new consumer is a conscious vocabulary change made here)"
    );
}

// ─────────── synthesized-carrier CONSTRUCTION-SITE inventory ────────────────
//
// The method inventory above pins the constructor VOCABULARY; this inventory
// pins every construction SITE — struct literals (`SynthesizedTemplateValue
// { … }` / `Self { … }` inside its own impl) and typed-constructor calls —
// across the whole runtime universe. A free-fn/string constructor, a struct
// literal outside the three constructors, or a new call site fails the
// equality gate until consciously classified here.

/// Every production `(impl self-type, fn name, body)` of a parsed file —
/// the impl-context-aware sibling of [`collect_production_fns`].
fn collect_production_fns_with_impl<'f>(
    items: &'f [syn::Item],
    out: &mut Vec<(Option<String>, String, &'f syn::Block)>,
) {
    for item in items {
        match item {
            syn::Item::Fn(f) if !is_cfg_test(&f.attrs) => {
                out.push((None, f.sig.ident.to_string(), &f.block));
            }
            syn::Item::Impl(imp) if !is_cfg_test(&imp.attrs) => {
                let self_ty = match imp.self_ty.as_ref() {
                    syn::Type::Path(p) => p.path.segments.last().map(|s| s.ident.to_string()),
                    _ => None,
                };
                for ii in &imp.items {
                    if let syn::ImplItem::Fn(f) = ii {
                        if !is_cfg_test(&f.attrs) {
                            out.push((self_ty.clone(), f.sig.ident.to_string(), &f.block));
                        }
                    }
                }
            }
            syn::Item::Mod(m) if !is_cfg_test(&m.attrs) => {
                if let Some((_, items)) = &m.content {
                    collect_production_fns_with_impl(items, out);
                }
            }
            _ => {}
        }
    }
}

/// A syn visitor collecting every CONSTRUCTION of the sealed carrier inside
/// one fn body: struct literals and `SynthesizedTemplateValue::<ctor>(…)` /
/// `Self::<ctor>(…)` (inside its own impl) path calls. Comments and strings
/// never count.
struct SynthesizedConstructionScan {
    in_own_impl: bool,
    hits: Vec<String>,
}

impl<'ast> syn::visit::Visit<'ast> for SynthesizedConstructionScan {
    fn visit_expr_struct(&mut self, node: &'ast syn::ExprStruct) {
        if let Some(seg) = node.path.segments.last() {
            let name = seg.ident.to_string();
            if name == "SynthesizedTemplateValue" || (name == "Self" && self.in_own_impl) {
                self.hits.push("literal".to_string());
            }
        }
        syn::visit::visit_expr_struct(self, node);
    }

    fn visit_expr_call(&mut self, node: &'ast syn::ExprCall) {
        if let syn::Expr::Path(p) = node.func.as_ref() {
            let segs: Vec<String> = p
                .path
                .segments
                .iter()
                .map(|s| s.ident.to_string())
                .collect();
            if segs.len() >= 2 {
                let ty = &segs[segs.len() - 2];
                if ty == "SynthesizedTemplateValue" || (ty == "Self" && self.in_own_impl) {
                    self.hits.push(format!("call:{}", segs[segs.len() - 1]));
                }
            }
        }
        syn::visit::visit_expr_call(self, node);
    }
}

/// Every `(enclosing fn, construction kind) -> count` of one parsed module.
fn synthesized_construction_sites(
    file: &syn::File,
) -> std::collections::BTreeMap<(String, String), usize> {
    use syn::visit::Visit;
    let mut out = std::collections::BTreeMap::new();
    let mut bodies = Vec::new();
    collect_production_fns_with_impl(&file.items, &mut bodies);
    for (impl_ty, fn_name, block) in bodies {
        let mut scan = SynthesizedConstructionScan {
            in_own_impl: impl_ty.as_deref() == Some("SynthesizedTemplateValue"),
            hits: Vec::new(),
        };
        scan.visit_block(block);
        for hit in scan.hits {
            *out.entry((fn_name.clone(), hit)).or_insert(0) += 1;
        }
    }
    out
}

/// The CLOSED pinned construction-site inventory: `(module, enclosing fn,
/// construction kind, count)`. Struct LITERALS exist only inside the three
/// typed constructors of the dedicated module; every planner construction is
/// one of the three typed-constructor calls.
const SYNTHESIZED_CONSTRUCTION_SITES: &[(&str, &str, &str, usize)] = &[
    (
        "client_plan_element_ops.rs",
        "project_set_class_pieces",
        "call:clsx",
        1,
    ),
    (
        "client_plan_spread_html.rs",
        "attribute_effect_items",
        "call:class_directives",
        1,
    ),
    (
        "client_plan_spread_html.rs",
        "attribute_effect_items",
        "call:style_directives",
        1,
    ),
    ("synthesized_value.rs", "clsx", "literal", 1),
    ("synthesized_value.rs", "class_directives", "literal", 1),
    ("synthesized_value.rs", "style_directives", "literal", 1),
];

#[test]
fn synthesized_construction_sites_are_pinned_to_the_sealed_module() {
    let mut discovered: std::collections::BTreeMap<(String, String, String), usize> =
        std::collections::BTreeMap::new();
    for module in runtime_modules() {
        for ((fn_name, kind), count) in
            synthesized_construction_sites(&parse_runtime_module(&module))
        {
            discovered.insert((module.clone(), fn_name, kind), count);
        }
    }
    let pinned: std::collections::BTreeMap<(String, String, String), usize> =
        SYNTHESIZED_CONSTRUCTION_SITES
            .iter()
            .map(|(m, f, k, n)| ((m.to_string(), f.to_string(), k.to_string()), *n))
            .collect();
    assert_eq!(
        discovered, pinned,
        "every SynthesizedTemplateValue construction site must be pinned in \
         SYNTHESIZED_CONSTRUCTION_SITES — a free-fn/string constructor, an \
         out-of-constructor struct literal, or a new call site is a conscious \
         vocabulary change made here"
    );
    // Table-integrity invariants (a wrong conscious edit is loud): literals
    // live ONLY in the dedicated module's three typed constructors; calls
    // name ONLY the three typed constructors.
    for (module, fn_name, kind, _) in SYNTHESIZED_CONSTRUCTION_SITES {
        if *kind == "literal" {
            assert_eq!(
                *module, SYNTHESIZED_OWNER,
                "a struct literal outside {SYNTHESIZED_OWNER} unseals the carrier"
            );
            assert!(
                ["clsx", "class_directives", "style_directives"].contains(fn_name),
                "a struct literal outside the three typed constructors \
                 unseals the carrier (found in `{fn_name}`)"
            );
        } else {
            assert!(
                [
                    "call:clsx",
                    "call:class_directives",
                    "call:style_directives"
                ]
                .contains(kind),
                "construction calls are limited to the three typed \
                 constructors (found `{kind}`)"
            );
        }
    }
    // Non-vacuity: the three constructors and at least one planner call are
    // both visible to the scan.
    assert!(
        discovered
            .keys()
            .any(|(m, _, k)| m == SYNTHESIZED_OWNER && k == "literal"),
        "the constructor literals must stay visible to the scan"
    );
    assert!(
        discovered.keys().any(|(_, _, k)| k.starts_with("call:")),
        "the planner constructor calls must stay visible to the scan"
    );
}

#[test]
fn construction_scan_discriminates_a_planted_free_ctor() {
    // Mutation: a free fn (or a new method) constructing the carrier from a
    // raw `String` — the exact masquerade lane the sealing exists to close.
    // The scan discovers the literal; the pinned inventory does not contain
    // it — the equality gate fails.
    let planted: syn::File = syn::parse_str(
        "pub(super) fn rogue_from_text(text: String) -> SynthesizedTemplateValue {\n\
             SynthesizedTemplateValue { text, has_call: false }\n\
         }",
    )
    .expect("planted snippet parses");
    let sites = synthesized_construction_sites(&planted);
    assert_eq!(
        sites.get(&("rogue_from_text".to_string(), "literal".to_string())),
        Some(&1),
        "the scan must discover a planted free-fn struct-literal constructor"
    );
    assert!(
        !SYNTHESIZED_CONSTRUCTION_SITES
            .iter()
            .any(|(_, f, _, _)| *f == "rogue_from_text"),
        "a planted free constructor must not be pre-classified"
    );
    // A planted NEW constructor method (`Self { … }` inside the impl) is
    // discovered through the impl context…
    let planted_method: syn::File = syn::parse_str(
        "impl SynthesizedTemplateValue {\n\
             pub(super) fn from_raw(text: String) -> Self {\n\
                 Self { text, has_call: false }\n\
             }\n\
         }",
    )
    .expect("planted method parses");
    let sites = synthesized_construction_sites(&planted_method);
    assert_eq!(
        sites.get(&("from_raw".to_string(), "literal".to_string())),
        Some(&1),
        "the scan must discover a planted `Self {{ … }}` constructor method"
    );
    // …and a planted out-of-module typed-constructor call is discovered as a
    // call site, while comment/string mentions never count.
    let commented: syn::File = syn::parse_str(
        "// SynthesizedTemplateValue { text } in prose\n\
         fn clean() { let s = \"SynthesizedTemplateValue { text }\"; let _ = s; }",
    )
    .expect("commented snippet parses");
    assert!(
        synthesized_construction_sites(&commented).is_empty(),
        "comment/string mentions must not count as construction sites"
    );
}

#[test]
fn impl_permit_list_discriminates_a_rogue_associated_const() {
    // The associated-const vector: an associated const typed
    // `fn(String) -> Self` constructs the sealed carrier from raw `text` inside
    // the fields' module-private scope. It is NOT an `ImplItem::Fn` (invisible
    // to the method inventory) and its initializer is NOT a fn body (invisible
    // to the construction-site scan). The associated-item permit-list must fail
    // closed on it.
    let rogue: syn::File = syn::parse_str(
        "impl SynthesizedTemplateValue {\n\
             pub(super) const ROGUE: fn(String) -> Self =\n\
                 |text| Self { text, has_call: false };\n\
         }",
    )
    .expect("rogue impl parses");
    let syn::Item::Impl(rogue_impl) = &rogue.items[0] else {
        panic!("expected an impl item");
    };
    let planted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        assert_synthesized_impl_items_sealed(rogue_impl)
    }));
    assert!(
        planted.is_err(),
        "the associated-item permit-list must fail closed on a rogue associated \
         const that struct-literals the sealed carrier"
    );
    // An unpinned METHOD is likewise rejected (the impl may not grow a new
    // constructor/accessor unreviewed).
    let rogue_method: syn::File = syn::parse_str(
        "impl SynthesizedTemplateValue {\n\
             pub(super) fn from_raw(text: String) -> Self { Self { text, has_call: false } }\n\
         }",
    )
    .expect("rogue method impl parses");
    let syn::Item::Impl(rogue_method_impl) = &rogue_method.items[0] else {
        panic!("expected an impl item");
    };
    let planted_method = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        assert_synthesized_impl_items_sealed(rogue_method_impl)
    }));
    assert!(
        planted_method.is_err(),
        "the associated-item permit-list must fail closed on an unpinned impl method"
    );
    // Control: the real sealed impl (three typed constructors + two raw-only
    // accessors) passes without panic, and the owner holds exactly one impl.
    let owner = parse_runtime_module(SYNTHESIZED_OWNER);
    let mut checked = 0usize;
    for item in &owner.items {
        if let syn::Item::Impl(imp) = item {
            assert_synthesized_impl_items_sealed(imp);
            checked += 1;
        }
    }
    assert_eq!(
        checked, 1,
        "the sealed owner module holds exactly one inherent impl"
    );
}

#[test]
fn owner_exposes_exactly_one_preparation_entry() {
    // ONE preparation entry over the closed `AuthoredValueInput` vocabulary —
    // a second `prepare_*` entry would re-open a parallel policy-selection
    // path (the retired `prepare_render_callee_slice` class).
    let owner = parse_runtime_module(OWNER);
    let entries: Vec<String> = impl_methods(&owner)
        .into_iter()
        .filter(|(name, is_pub)| *is_pub && name.starts_with("prepare_"))
        .map(|(name, _)| name)
        .collect();
    assert_eq!(
        entries,
        vec!["prepare_template_value".to_string()],
        "client_legacy_value.rs must expose exactly ONE preparation entry"
    );
    // The closed input vocabulary exists and stays closed (two arms: the
    // analyzed expression + the render-callee slice).
    let input = find_enum(&owner, "AuthoredValueInput").expect("AuthoredValueInput enum");
    let arms: Vec<String> = input.variants.iter().map(|v| v.ident.to_string()).collect();
    assert_eq!(
        arms,
        vec!["Expr".to_string(), "RenderCalleeSlice".to_string()],
        "the authored-input vocabulary is the closed two-arm enum \
         (adding an arm is a conscious vocabulary change, reviewed here)"
    );
}

#[test]
fn no_planner_or_emitter_bypasses_the_preparation_entry() {
    // AST-level bypass detection (whitespace-proof, comment-proof): outside
    // the owner and the printer's defining module, NO runtime module may
    // reference the low-level `ExprId -> String` value printer — preparation
    // is the only route to a serialized authored value.
    for module in runtime_modules() {
        if module == OWNER || module == PRINTER_DEFINER {
            continue;
        }
        let file = parse_runtime_module(&module);
        assert!(
            !has_ident(&file, "rewrite_value_preserving_source"),
            "{module} must route authored values through prepare_template_value, \
             not the low-level value printer"
        );
    }
    // Scan non-vacuity.
    assert!(
        runtime_modules().len() >= 20,
        "the runtime module universe must stay non-vacuous"
    );
}

// ─────── RAIL 2: expression-serialization call-site classification ──────────
//
// The value printer (`rewrite_value_preserving_source`) is not the only way to
// serialize an authored `ExprId`/source: the GENERAL rewrite family
// (`self.rewrite`, `rewrite_source`, the statement/rune/plain-JS lanes, and
// the underlying `expr_rewrite` entry functions) also yields emitted
// expression text, bypassing `prepare_template_value` if called on an
// authored-VALUE position. This inventory DISCOVERS every such call site
// structurally (syn AST — method calls + path calls, closures included) and
// requires each to be an EXPLICITLY-CLASSIFIED non-authored-value role. A new
// call site fails the gate until consciously classified here.

/// The expression-serialization FAMILY: every inherent printer method the
/// printer module defines PLUS every public rewriter entry of the underlying
/// `expr_rewrite` module — discovered STRUCTURALLY from their definitions, so
/// a new sibling printer method / rewriter entry joins the scan automatically.
fn serialization_family() -> BTreeSet<String> {
    let mut family: BTreeSet<String> = impl_methods(&parse_runtime_module(PRINTER_DEFINER))
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    let rewriter = parse_runtime_module("expr_rewrite/mod.rs");
    for item in &rewriter.items {
        if let syn::Item::Fn(f) = item {
            if !matches!(f.vis, syn::Visibility::Inherited) {
                family.insert(f.sig.ident.to_string());
            }
        }
    }
    family
}

/// A syn visitor collecting every CALL of a family entry: a method call
/// (`recv.rewrite(…)`) or a path call (`expr_rewrite::rewrite_expression_full(…)`,
/// `Self::rewrite(…)`) whose callee name is in the family. Bodies are walked
/// in full (nested closures/blocks included); comments and strings never count.
struct SerializationCallScan<'a> {
    family: &'a BTreeSet<String>,
    hits: Vec<String>,
}

impl<'ast> syn::visit::Visit<'ast> for SerializationCallScan<'_> {
    fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
        let name = node.method.to_string();
        if self.family.contains(&name) {
            self.hits.push(name);
        }
        syn::visit::visit_expr_method_call(self, node);
    }

    fn visit_expr_call(&mut self, node: &'ast syn::ExprCall) {
        if let syn::Expr::Path(p) = node.func.as_ref() {
            if let Some(seg) = p.path.segments.last() {
                let name = seg.ident.to_string();
                if self.family.contains(&name) {
                    self.hits.push(name);
                }
            }
        }
        syn::visit::visit_expr_call(self, node);
    }
}

/// Whether an item is gated by EXACTLY `#[cfg(test)]` — a `cfg` list whose
/// single nested predicate is the bare path `test`, matched structurally on
/// the parsed `syn::Meta` (never a token-spelling scan). Unit-test code
/// exercises the printer/rewriter/preparation entries directly by design — it
/// is not production routing. EVERY other predicate — `not(test)`,
/// `any(test, …)`, `all(test, …)`, `feature = "…"`, any compound/negated
/// form — can compile in a production configuration, so it classifies
/// CONSERVATIVELY as PRODUCTION and every rail scans the item. This is the
/// SOLE test-only classifier for the module-graph traversal and the fn/impl
/// inventories.
fn is_cfg_test(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|a| {
        if !a.path().is_ident("cfg") {
            return false;
        }
        let Ok(preds) = a.parse_args_with(
            syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
        ) else {
            return false;
        };
        preds.len() == 1 && matches!(&preds[0], syn::Meta::Path(p) if p.is_ident("test"))
    })
}

/// Every PRODUCTION fn of a parsed file as `(name, signature, body)` — free
/// fns, inherent/trait impl fns, and fns inside inline non-test `mod` blocks,
/// recursively. `#[cfg(test)]`-gated mods/impls/fns are skipped, so a fn
/// cannot hide from the inventories inside an inline production module.
fn collect_production_fns<'f>(
    items: &'f [syn::Item],
    out: &mut Vec<(String, &'f syn::Signature, &'f syn::Block)>,
) {
    for item in items {
        match item {
            syn::Item::Fn(f) if !is_cfg_test(&f.attrs) => {
                out.push((f.sig.ident.to_string(), &f.sig, &f.block));
            }
            syn::Item::Impl(imp) if !is_cfg_test(&imp.attrs) => {
                for ii in &imp.items {
                    if let syn::ImplItem::Fn(f) = ii {
                        if !is_cfg_test(&f.attrs) {
                            out.push((f.sig.ident.to_string(), &f.sig, &f.block));
                        }
                    }
                }
            }
            syn::Item::Mod(m) if !is_cfg_test(&m.attrs) => {
                if let Some((_, items)) = &m.content {
                    collect_production_fns(items, out);
                }
            }
            _ => {}
        }
    }
}

/// Every `(enclosing fn, family entry) -> call count` of one parsed module
/// (production fns only — inline non-test mods walked, cfg(test) skipped).
fn serialization_sites(
    file: &syn::File,
    family: &BTreeSet<String>,
) -> std::collections::BTreeMap<(String, String), usize> {
    use syn::visit::Visit;
    let mut out = std::collections::BTreeMap::new();
    let mut bodies = Vec::new();
    collect_production_fns(&file.items, &mut bodies);
    for (fn_name, _sig, block) in bodies {
        let mut scan = SerializationCallScan {
            family,
            hits: Vec::new(),
        };
        scan.visit_block(block);
        for hit in scan.hits {
            *out.entry((fn_name.clone(), hit)).or_insert(0) += 1;
        }
    }
    out
}

/// The CLOSED classified inventory: `(module, enclosing fn, family entry,
/// call count, non-authored-value role)`. Every row is a conscious ruling
/// that the call site serializes a NON-authored-value role (an lvalue, a
/// bind get/set pair, a callee/function reference, a dep-read name, the `$:`
/// statement lane, or the preparation entry itself). An authored template
/// VALUE must instead route through `prepare_template_value`.
const CLASSIFIED_SERIALIZATION_SITES: &[(&str, &str, &str, usize, &str)] = &[
    (
        "client_block_plan.rs",
        "project_declaration_tag",
        "rewrite",
        1,
        "template-rune `$derived` declarator INIT — the runes-only declaration \
         lane (official lowers it via `$.derived(() => …)`, never \
         `build_expression`; plain declaration-tag initializers route through \
         the DeclarationTagInitializer surface)",
    ),
    (
        "client_component_plan.rs",
        "project_bind_prop",
        "rewrite_source",
        2,
        "component `bind:` get/set pair — official builds getter + setter from \
         the BOUND expression (an lvalue role, never `build_expression`)",
    ),
    (
        "client_component_plan.rs",
        "project_bind_prop",
        "rewrite_source_plain_js",
        2,
        "component function-pair bind `{get, set}` ELEMENTS (the plain-JS \
         lane over pre-extracted pair slices)",
    ),
    (
        "client_component_plan.rs",
        "project_bind_this",
        "rewrite_source_plain_js",
        2,
        "component `bind:this` function-pair `{get, set}` elements",
    ),
    (
        "client_component_plan.rs",
        "render_dynamic_callee",
        "rewrite_source",
        1,
        "the `{@render}` dynamic-callee SLICE rewrite — the rewritten input \
         handed to the RenderCalleeSlice preparation arm (the wrap decision \
         itself routes through prepare_template_value)",
    ),
    (
        "client_legacy_value.rs",
        "legacy_wrap_rewritten",
        "rewrite_source",
        2,
        "the wrap core's visible dep-READ names (`$.deep_read_state(read)` / \
         plain reads) — single reference identifiers, not authored values",
    ),
    (
        "client_legacy_value.rs",
        "prepare_template_value",
        "rewrite_value_preserving_source",
        1,
        "the sole preparation entry itself (the owner's value-printer call)",
    ),
    (
        "client_legacy_value.rs",
        "unthunk_callee",
        "rewrite_source",
        2,
        "the `b.thunk` unthunk-callee NAME rewrites (a zero-arg callee / a \
         bare-identifier accessor read, each a single identifier compared \
         against its constructed form — not authored values)",
    ),
    (
        "client_plan.rs",
        "project_scope_op",
        "rewrite",
        1,
        "`use:` action CALLEE — a function reference (official visits the \
         action expression without `build_expression`; the argument routes \
         through the UseActionArg surface)",
    ),
    (
        "client_plan.rs",
        "project_scope_op",
        "rewrite_source",
        2,
        "transition/animation function NAMES (the `() => fn` getter \
         references; params route through TransitionParams/AnimationParams)",
    ),
    (
        "client_plan_bind.rs",
        "bind_getter_setter",
        "rewrite",
        2,
        "DOM `bind:` getter + member-lvalue setter — read/write-target roles \
         of the bound expression, never `build_expression`",
    ),
    (
        "client_plan_bind.rs",
        "bind_getter_setter",
        "rewrite_source_plain_js",
        2,
        "DOM function-pair bind `{get, set}` elements (the plain-JS lane)",
    ),
    (
        "client_plan_script.rs",
        "build_script_items",
        "rewrite_source",
        5,
        "instance-script declaration/effect carriers — the SCRIPT statement \
         lane, not template values",
    ),
    (
        "client_plan_script.rs",
        "build_script_items",
        "rewrite_statement_source",
        1,
        "the top-level user-effect expression STATEMENT lane",
    ),
    (
        "client_plan_script.rs",
        "build_script_items",
        "rewrite_script_statement",
        1,
        "ordinary instance-script statements rewritten from the canonical \
         parsed program (the script statement lane, not template values)",
    ),
    (
        "client_plan_script.rs",
        "build_script_items",
        "rewrite_rune_init_source",
        1,
        "the effect-rune declarator INIT lane",
    ),
    (
        "client_plan_script.rs",
        "lower_props_default",
        "rewrite_source",
        2,
        "legacy prop DEFAULT initializers (script lane)",
    ),
    (
        "client_plan_script.rs",
        "build_module_statements",
        "rewrite_script_statement",
        1,
        "ordinary module-script statements rewritten from the canonical \
         parsed program (the script statement lane, not template values)",
    ),
    (
        "client_plan_script.rs",
        "lower_reactive_deps_thunk",
        "rewrite_source",
        3,
        "`$:` reactive-statement dependency READ names (the \
         `$.legacy_pre_effect` deps thunk — a DIFFERENT official feature from \
         the template value wrap)",
    ),
    (
        "client_plan_script.rs",
        "lower_reactive_statements",
        "rewrite_source",
        2,
        "`$:` reactive statement BODIES (script statement lane)",
    ),
    (
        "client_plan_spread_html.rs",
        "rewrite_identifier",
        "rewrite_expression_full",
        1,
        "the `{@html}` elision callee-NAME rewrite — a single identifier \
         compared against its bare name (the payload itself was PREPARED \
         first through prepare_template_value)",
    ),
    (
        "expr_rewrite/mod.rs",
        "rewrite_expression",
        "rewrite_expression_with_props",
        1,
        "the public rewriter entry's own delegation to its props-aware \
         sibling — the serialization boundary's internal composition inside \
         the family-defining module, not a planner call site",
    ),
];

#[test]
fn every_expression_serialization_call_site_is_classified() {
    let family = serialization_family();
    // Family non-vacuity: the printer methods + the underlying rewriter
    // entries are both present (an empty family would scan vacuously).
    for core in [
        "rewrite",
        "rewrite_source",
        "rewrite_value_preserving_source",
        "rewrite_expression_full",
    ] {
        assert!(family.contains(core), "family must contain `{core}`");
    }
    let mut discovered: std::collections::BTreeMap<(String, String, String), usize> =
        std::collections::BTreeMap::new();
    for module in runtime_modules() {
        if module == PRINTER_DEFINER {
            // The printer's defining module delegates to the `expr_rewrite`
            // entries by design — it IS the serialization boundary. Its own
            // method inventory is covered by `serialization_family()`.
            continue;
        }
        for ((fn_name, method), count) in
            serialization_sites(&parse_runtime_module(&module), &family)
        {
            discovered.insert((module.clone(), fn_name, method), count);
        }
    }
    let classified: std::collections::BTreeMap<(String, String, String), usize> =
        CLASSIFIED_SERIALIZATION_SITES
            .iter()
            .map(|(module, fn_name, method, count, _role)| {
                (
                    (module.to_string(), fn_name.to_string(), method.to_string()),
                    *count,
                )
            })
            .collect();
    assert_eq!(
        discovered, classified,
        "every expression-serialization call site must be explicitly \
         classified as a non-authored-value role in \
         CLASSIFIED_SERIALIZATION_SITES (and stale rows must be removed): an \
         authored template VALUE routes through prepare_template_value, never \
         a direct rewrite"
    );
}

#[test]
fn wrap_trigger_thunk_and_render_facts_come_from_canonical_analysis_not_reparse() {
    // What this test PROVES: the three retired PLANNER-LOCAL fact-recovery
    // reparse sites stay deleted — wrap trigger facts, thunk topology, and
    // the dynamic-`{@render}` callee lowering consume the canonical
    // trigger/reference/thunk facts populated by the canonical expression
    // analysis, and `render_dynamic_callee` hosts no direct planner-local
    // parser. What it does NOT prove: the absence of reparsing across the
    // CALLED path — `prepare_template_value` still invokes the scope-aware
    // `expr_has_call` / `expr_has_binding_impurity` over the callee slice
    // (`client_legacy_value.rs`), the ratified fail-closed D-60 residual
    // (every recovery failure surfaces as
    // `svelte-runtime-unsupported-expression-fact-recovery`, never a raw /
    // empty degrade), eliminated only by the D-60 typed-IR carrier boundary.
    //
    // Site 1: the sync member/assignment TRIGGER fact is populated on
    // `AnalyzedExpr` — the per-wrap reparse helper is deleted.
    let reactive = parse_runtime_module("reactive_analysis.rs");
    assert!(
        !has_ident(&reactive, "expr_has_sync_member_or_assignment"),
        "reactive_analysis.rs must not re-derive the member/assignment wrap \
         trigger by reparse (the fact lives on AnalyzedExpr)"
    );
    // Site 2: the thunk-topology decision reads the analyzed
    // zero-arg-callee fact — no OXC parse of GENERATED text remains in the
    // codegen helpers.
    let helpers = parse_runtime_module("client_codegen_helpers.rs");
    for banned in ["oxc_parser", "Parser", "zero_arg_ident_call_callee"] {
        assert!(
            !has_ident(&helpers, banned),
            "client_codegen_helpers.rs must not parse generated text for \
             thunk topology (`{banned}`)"
        );
    }
    // Site 3: the dynamic `{@render}` callee lowering consumes the populated
    // span/shape/reference facts — no direct planner-LOCAL parser in
    // `render_dynamic_callee`, no re-collected references, no
    // `unwrap_or_default` reference fallback (an empty-reference degrade
    // silently drops wrap deps). The called preparation path's callee-slice
    // `expr_has_call` / impurity reparses remain (the D-60 residual above).
    let component = parse_runtime_module("client_component_plan.rs");
    let render_fn_idents: Vec<String> = component
        .items
        .iter()
        .filter_map(|item| match item {
            syn::Item::Impl(imp) => Some(imp),
            _ => None,
        })
        .flat_map(|imp| &imp.items)
        .filter_map(|ii| match ii {
            syn::ImplItem::Fn(f) if f.sig.ident == "render_dynamic_callee" => Some(f),
            _ => None,
        })
        .flat_map(|f| {
            let mut out = Vec::new();
            collect_idents(f.block.to_token_stream(), &mut out);
            out
        })
        .collect();
    assert!(
        !render_fn_idents.is_empty(),
        "render_dynamic_callee must exist in client_component_plan.rs"
    );
    for banned in ["Parser", "collect_expr_references", "unwrap_or_default"] {
        assert!(
            !render_fn_idents.iter().any(|i| i == banned),
            "render_dynamic_callee must consume the populated render-callee \
             facts, not `{banned}`"
        );
    }
}

#[test]
fn serialization_scan_discriminates_a_planted_general_rewrite_bypass() {
    // A planted general `self.rewrite(expr, scope)?` in an authored-value
    // planner must be DISCOVERED by the scan and must NOT satisfy the
    // classified inventory — the classified-inventory equality gate fails on
    // it.
    let planted: syn::File = syn::parse_str(
        "impl<'a> SupportedClientIr<'a> {\n\
             fn sneaky_value(&self, expr: ExprId, scope: ScopeId) -> Result<String, E> {\n\
                 let v = self.rewrite(expr, scope)?;\n\
                 Ok(v)\n\
             }\n\
         }",
    )
    .expect("planted snippet parses");
    let family = serialization_family();
    let sites = serialization_sites(&planted, &family);
    assert_eq!(
        sites.get(&("sneaky_value".to_string(), "rewrite".to_string())),
        Some(&1),
        "the scan must discover the planted general-rewrite call"
    );
    // The planted site is unclassified for EVERY authored-value planner
    // module — the inventory equality would fail regardless of which planner
    // hosts it.
    for planner in AUTHORED_VALUE_PLANNERS {
        assert!(
            !CLASSIFIED_SERIALIZATION_SITES
                .iter()
                .any(|(m, f, me, _, _)| {
                    m == planner && *f == "sneaky_value" && *me == "rewrite"
                }),
            "a planted bypass must not be pre-classified"
        );
    }
    // And a comment/string mention is NOT a call.
    let commented: syn::File = syn::parse_str(
        "// self.rewrite(expr, scope) in prose\n\
         fn clean() { let s = \"self.rewrite(expr, scope)\"; let _ = s; }",
    )
    .expect("commented snippet parses");
    assert!(
        serialization_sites(&commented, &family).is_empty(),
        "comment/string mentions must not count as serialization calls"
    );
}

#[test]
fn authored_expr_consumers_are_exactly_the_closed_planner_inventory() {
    // Structural consumer discovery: a module consumes `AuthoredExpr` iff the
    // ident appears in its AST (never in comments/strings — syn drops those).
    let mut consumers: BTreeSet<String> = BTreeSet::new();
    for module in runtime_modules() {
        if module == OWNER {
            continue;
        }
        if has_ident(&parse_runtime_module(&module), "AuthoredExpr") {
            consumers.insert(module);
        }
    }
    let expected: BTreeSet<String> = AUTHORED_VALUE_PLANNERS
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(
        consumers, expected,
        "the AuthoredExpr consumer set must equal the closed planner inventory \
         (a new consumer is a conscious vocabulary change made in this guard)"
    );
}

/// EXACT normalized-type equality — substring matching would let a tuple
/// `(PreparedTemplateValue, String)`, a `PreparedTemplateValueBypass` alias,
/// or an unexpected wrapper restore a raw-text lane while still "containing"
/// the carrier name.
fn carrier_type_is_pinned(found: &str, want: &str) -> bool {
    found == want
}

#[test]
fn carrier_type_pin_discriminates_tuple_and_alias_bypasses() {
    assert!(carrier_type_is_pinned(
        "PreparedTemplateValue",
        "PreparedTemplateValue"
    ));
    for bypass in [
        "(PreparedTemplateValue,String)",
        "PreparedTemplateValueBypass",
        "Vec<PreparedTemplateValue>",
        "Option<PreparedTemplateValue>",
    ] {
        assert!(
            !carrier_type_is_pinned(bypass, "PreparedTemplateValue"),
            "exact pin must reject the substring-bypass `{bypass}`"
        );
    }
}

#[test]
fn narrow_plan_value_fields_are_typed_prepared_carriers_not_strings() {
    // The expression-value fields of the narrow plan are the TYPED prepared
    // carriers (constructible only through the sole entry / synthesis
    // constructor) — never bare `String`s an emitter could splice.
    let types = container_fields(&parse_runtime_module("client_plan_types.rs"));
    let blocks = container_fields(&parse_runtime_module("client_plan_block_types.rs"));
    let all: Vec<&(String, String, String)> = types.iter().chain(blocks.iter()).collect();
    for (container, field, want) in [
        (
            "ClientRuntimeOp::ReactiveText",
            "value",
            "PreparedTemplateValue",
        ),
        (
            "ClientRuntimeOp::AttributeEffect",
            "items",
            "Vec<AttributeEffectItem>",
        ),
        ("ClientRuntimeOp::Html", "payload", "PreparedTemplateValue"),
        ("ClientRuntimeOp::Html", "getter_form", "HtmlGetterForm"),
        ("AttrValuePart::Expr", "value", "PreparedTemplateValue"),
        ("AttrValue::Single", "0", "PlannedTemplateValue"),
        (
            "PlannedTemplateValue::Authored",
            "0",
            "PreparedTemplateValue",
        ),
        (
            "PlannedTemplateValue::Synthesized",
            "0",
            "SynthesizedTemplateValue",
        ),
        ("BoundaryAttrProp", "value", "PreparedTemplateValue"),
        (
            "ElementLifecycleOp::Attachment",
            "payload",
            "PreparedTemplateValue",
        ),
        ("ClientIfBranch", "test", "Option<PreparedIfCondition>"),
        ("ClientEach", "source", "PreparedTemplateValue"),
        ("ClientEachKey", "expr", "PreparedTemplateValue"),
        ("ClientAwait", "promise", "PreparedTemplateValue"),
        ("ClientBlock::Key", "expr", "PreparedTemplateValue"),
        (
            "ClientDeclaration::Derived",
            "init",
            "PreparedTemplateValue",
        ),
        ("ClientDeclaration::Derived", "helper", "DerivedHelper"),
    ] {
        let ty = all
            .iter()
            .find(|(c, f, _)| c == container && f == field)
            .unwrap_or_else(|| panic!("carrier field {container}.{field} must exist"));
        assert!(
            carrier_type_is_pinned(ty.2.as_str(), want),
            "{container}.{field} must stay EXACTLY the typed carrier `{want}` \
             (found `{}`) — a tuple/alias/wrapper around the carrier is a \
             raw-text lane, not a match",
            ty.2
        );
    }
    // Structural NEGATIVE: the retired parallel-carrier field names stay
    // deleted from the plan-type modules (no `legacy_wrapped` side-channel, no
    // pre-flattened fold body).
    for (container, field, _) in all {
        assert!(
            field != "legacy_wrapped" && field != "fold_body",
            "{container}.{field}: the retired parallel-carrier shape must stay deleted"
        );
    }
}

// ─────────── RAIL 3: authored-position → surface binding inventory ──────────
//
// The serialization inventory above proves no KNOWN-family call site
// SERIALIZES an authored value outside the preparation entry (fail-closed
// for the inventoried rewrite family) — but it cannot see a call
// site that DOES route through `prepare_template_value` while selecting the
// WRONG `AuthoredValueSurface`: a new authored position passing an existing
// Raw-classified surface (e.g. `EventHandler`) for a should-wrap value emits
// raw while every structural/freshness test above stays green — a silent
// future fail-open. This inventory HARDENS that class as a fail-closed
// SECONDARY tripwire over the discovered surface-accepting call sites (NOT a
// completeness proof — the transparent-alias / method-body-construction
// residual stays D-61-deferred): every CALL of a
// surface-accepting fn (any production fn a parameter of which is written with
// `AuthoredValueSurface` in its type spelling — the preparation entry, the
// policy table, and every forwarding helper, discovered by a FAIL-CLOSED
// substring scan so a new forwarding layer auto-joins it) is pinned to its
// REQUIRED surface binding. The scan is a substring match, NOT full type
// resolution: a transparent alias `type Surface = AuthoredValueSurface` that
// renames the type away from the spelling is not discovered — that residual,
// like an in-owner permitted method body constructing the sealed carrier at
// runtime, is the ratified D-61 backend-wide authored-emission capability
// boundary, not chased here. A new position, a surface
// swap at an existing position, a laundered local
// (`let s = …; prepare_template_value(_, s)`), or an unpinned forwarding
// caller fails the equality gate until consciously classified here.

/// The surface-accepting fn inventory, discovered structurally:
/// `(module, fn) -> (surface param index among call arguments, param name)`.
fn surface_accepting_fns() -> std::collections::BTreeMap<(String, String), (usize, String)> {
    let mut out = std::collections::BTreeMap::new();
    for module in runtime_modules() {
        let file = parse_runtime_module(&module);
        let mut fns = Vec::new();
        collect_production_fns(&file.items, &mut fns);
        for (name, sig, _block) in fns {
            let mut idx = 0usize;
            for input in &sig.inputs {
                let syn::FnArg::Typed(pat_ty) = input else {
                    continue;
                };
                let ty = pat_ty.ty.to_token_stream().to_string().replace(' ', "");
                if ty.contains("AuthoredValueSurface") {
                    let param = match pat_ty.pat.as_ref() {
                        syn::Pat::Ident(p) => p.ident.to_string(),
                        other => other.to_token_stream().to_string(),
                    };
                    out.insert((module.clone(), name.clone()), (idx, param));
                }
                idx += 1;
            }
        }
    }
    out
}

/// The CLOSED pinned inventory of surface-accepting fns:
/// `(module, fn, surface arg index, surface param name)`. Adding a forwarding
/// layer is a conscious vocabulary change, made here.
const SURFACE_ACCEPTING_FNS: &[(&str, &str, usize, &str)] = &[
    ("client_legacy_value.rs", "policy", 0, "surface"),
    (
        "client_legacy_value.rs",
        "prepare_template_value",
        1,
        "surface",
    ),
    ("client_plan_attr_value.rs", "attr_value_for", 2, "surface"),
    (
        "client_plan_attr_value.rs",
        "mixed_attr_value",
        1,
        "surface",
    ),
];

/// Describe a surface ARGUMENT expression. Recognized shapes: a literal
/// variant path (`…AuthoredValueSurface::EventHandler` → `"EventHandler"`)
/// and the enclosing forwarder's own surface parameter (`surface` →
/// `"forwarded(surface)"`). EVERYTHING else — a laundered local binding, a
/// call, a field read — renders as `unrecognized(…)`, which never matches a
/// pinned binding: unrecognized shapes fail closed.
fn describe_surface_arg(expr: &syn::Expr, forward_param: Option<&str>) -> String {
    if let syn::Expr::Path(p) = expr {
        let segs: Vec<String> = p
            .path
            .segments
            .iter()
            .map(|s| s.ident.to_string())
            .collect();
        if segs.len() >= 2 && segs[segs.len() - 2] == "AuthoredValueSurface" {
            return segs.last().unwrap().clone();
        }
        if let [single] = segs.as_slice() {
            if Some(single.as_str()) == forward_param {
                return format!("forwarded({single})");
            }
        }
    }
    format!(
        "unrecognized({})",
        expr.to_token_stream().to_string().replace(' ', "")
    )
}

/// A syn visitor collecting every call of a surface-accepting fn — method
/// calls and path calls, nested closures/blocks included — with the surface
/// argument described per [`describe_surface_arg`].
struct AuthoredPositionScan<'a> {
    by_name: &'a std::collections::BTreeMap<String, (usize, String)>,
    forward_param: Option<&'a str>,
    hits: Vec<(String, String)>,
}

impl AuthoredPositionScan<'_> {
    fn record<'ast>(
        &mut self,
        name: String,
        mut args: impl Iterator<Item = &'ast syn::Expr>,
        idx: usize,
    ) {
        let binding = args
            .nth(idx)
            .map(|arg| describe_surface_arg(arg, self.forward_param))
            .unwrap_or_else(|| "arity-mismatch".to_string());
        self.hits.push((name, binding));
    }
}

impl<'ast> syn::visit::Visit<'ast> for AuthoredPositionScan<'_> {
    fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
        let name = node.method.to_string();
        if let Some((idx, _)) = self.by_name.get(&name).cloned() {
            self.record(name, node.args.iter(), idx);
        }
        syn::visit::visit_expr_method_call(self, node);
    }

    fn visit_expr_call(&mut self, node: &'ast syn::ExprCall) {
        if let syn::Expr::Path(p) = node.func.as_ref() {
            if let Some(seg) = p.path.segments.last() {
                let name = seg.ident.to_string();
                if let Some((idx, _)) = self.by_name.get(&name).cloned() {
                    self.record(name, node.args.iter(), idx);
                }
            }
        }
        syn::visit::visit_expr_call(self, node);
    }
}

/// Every `(module, enclosing fn, callee, surface binding) -> call count` of
/// one parsed module.
fn authored_position_sites(
    module: &str,
    file: &syn::File,
    by_name: &std::collections::BTreeMap<String, (usize, String)>,
    module_forwarders: &std::collections::BTreeMap<String, String>,
) -> std::collections::BTreeMap<(String, String, String, String), usize> {
    use syn::visit::Visit;
    let mut out = std::collections::BTreeMap::new();
    let mut bodies = Vec::new();
    collect_production_fns(&file.items, &mut bodies);
    for (fn_name, _sig, block) in bodies {
        let mut scan = AuthoredPositionScan {
            by_name,
            forward_param: module_forwarders.get(&fn_name).map(String::as_str),
            hits: Vec::new(),
        };
        scan.visit_block(block);
        for (callee, binding) in scan.hits {
            *out.entry((module.to_string(), fn_name.clone(), callee, binding))
                .or_insert(0) += 1;
        }
    }
    out
}

/// The CLOSED pinned authored-position inventory: `(module, enclosing fn,
/// callee, REQUIRED surface binding, call count)`. Every row is a conscious
/// ruling that the position's surface classification is correct — verified
/// against the official compiler's per-position `build_expression` policy. A
/// new authored position, a surface swap, or a count drift fails the gate
/// until consciously (re-)classified here.
#[rustfmt::skip]
const AUTHORED_POSITION_SURFACE_BINDINGS: &[(&str, &str, &str, &str, usize)] = &[
    ("client_block_plan.rs", "project_block", "prepare_template_value", "AwaitPromise", 1),
    ("client_block_plan.rs", "project_block", "prepare_template_value", "KeyExpression", 1),
    ("client_block_plan.rs", "project_const_tag", "prepare_template_value", "ConstInitializer", 1),
    ("client_block_plan.rs", "project_debug_tag", "prepare_template_value", "DebugArg", 1),
    ("client_block_plan.rs", "project_declaration_tag", "prepare_template_value", "DeclarationTagInitializer", 1),
    ("client_block_plan.rs", "project_each", "prepare_template_value", "EachCollection", 1),
    ("client_block_plan.rs", "project_each", "prepare_template_value", "EachKeyExpression", 1),
    ("client_block_plan.rs", "project_if_branches", "prepare_template_value", "IfCondition", 1),
    ("client_component_plan.rs", "project_component_call", "prepare_template_value", "EventHandler", 1),
    ("client_component_plan.rs", "project_dynamic_prop", "prepare_template_value", "ComponentProp", 1),
    ("client_component_plan.rs", "project_prop_member", "mixed_attr_value", "ComponentProp", 1),
    ("client_component_plan.rs", "project_render", "prepare_template_value", "RenderArg", 1),
    ("client_component_plan.rs", "project_special_component", "prepare_template_value", "ComponentSelector", 1),
    ("client_component_plan.rs", "project_spread_arg", "prepare_template_value", "ComponentSpreadOperand", 1),
    ("client_component_plan.rs", "render_dynamic_callee", "prepare_template_value", "RenderCallee", 1),
    ("client_legacy_value.rs", "prepare_template_value", "policy", "forwarded(surface)", 1),
    // File ownership moved under the production-size split; the function,
    // authored-value callee, required surface, and call count are unchanged.
    ("client_plan_group_value.rs", "collect_group_dynamic_values", "attr_value_for", "AttributeValue", 1),
    ("client_plan.rs", "project_scope_op", "prepare_template_value", "AnimationParams", 1),
    ("client_plan.rs", "project_scope_op", "prepare_template_value", "AttachPayload", 1),
    ("client_plan.rs", "build_interpolation_plans", "prepare_template_value", "ReactiveText", 1),
    ("client_plan.rs", "project_scope_op", "prepare_template_value", "TransitionParams", 1),
    ("client_plan.rs", "project_scope_op", "prepare_template_value", "UseActionArg", 1),
    ("client_plan_attr_value.rs", "attr_value_for", "mixed_attr_value", "forwarded(surface)", 1),
    ("client_plan_attr_value.rs", "attr_value_for", "prepare_template_value", "forwarded(surface)", 1),
    ("client_plan_attr_value.rs", "mixed_attr_value", "prepare_template_value", "forwarded(surface)", 2),
    ("client_plan_bind.rs", "project_event_op", "prepare_template_value", "EventHandler", 1),
    ("client_plan_element_ops.rs", "non_static_property_value", "mixed_attr_value", "AttributeValue", 1),
    ("client_plan_element_ops.rs", "non_static_property_value", "prepare_template_value", "AttributeValue", 1),
    ("client_plan_element_ops.rs", "project_reactive_attr_op", "attr_value_for", "AttributeValue", 1),
    ("client_plan_element_ops.rs", "project_set_class_pieces", "mixed_attr_value", "ClassBase", 1),
    ("client_plan_element_ops.rs", "project_set_class_pieces", "prepare_template_value", "ClassBase", 1),
    ("client_plan_element_ops.rs", "project_set_class_pieces", "prepare_template_value", "ClassDirectiveCondition", 1),
    ("client_plan_element_ops.rs", "project_set_style_op", "mixed_attr_value", "StyleBase", 1),
    ("client_plan_element_ops.rs", "project_set_style_op", "mixed_attr_value", "StyleDirectiveValue", 1),
    ("client_plan_element_ops.rs", "project_set_style_op", "prepare_template_value", "StyleBase", 1),
    ("client_plan_element_ops.rs", "project_set_style_op", "prepare_template_value", "StyleDirectiveValue", 1),
    ("client_plan_spread_html.rs", "attribute_effect_items", "mixed_attr_value", "AttributeEffectValue", 1),
    ("client_plan_spread_html.rs", "attribute_effect_items", "mixed_attr_value", "StyleDirectiveValue", 1),
    ("client_plan_spread_html.rs", "attribute_effect_items", "prepare_template_value", "AttributeEffectValue", 1),
    ("client_plan_spread_html.rs", "attribute_effect_items", "prepare_template_value", "ClassDirectiveCondition", 1),
    ("client_plan_spread_html.rs", "attribute_effect_items", "prepare_template_value", "ElementSpreadOperand", 1),
    ("client_plan_spread_html.rs", "attribute_effect_items", "prepare_template_value", "StyleDirectiveValue", 1),
    ("client_plan_spread_html.rs", "project_html_op", "prepare_template_value", "HtmlPayload", 1),
    ("client_slot_plan.rs", "project_slot", "mixed_attr_value", "SlotProp", 1),
    ("client_slot_plan.rs", "project_slot", "prepare_template_value", "SlotSpreadOperand", 1),
    ("client_slot_plan.rs", "project_slot_dynamic_prop", "prepare_template_value", "SlotProp", 1),
    ("client_svelte_boundary.rs", "boundary_attr_prop", "prepare_template_value", "BoundaryProp", 1),
    ("client_svelte_element.rs", "project_svelte_element", "prepare_template_value", "EventHandler", 1),
    ("client_svelte_element.rs", "project_svelte_element", "prepare_template_value", "SvelteElementThis", 1),
    ("client_svelte_head.rs", "build_multi_chunk_title", "prepare_template_value", "TitleChunk", 1),
    ("client_svelte_head.rs", "build_single_expr_title", "prepare_template_value", "TitleChunk", 1),
];

/// The cross-module `name -> (surface arg index, param name)` view of the
/// acceptor inventory, asserting bare-name consistency (the call scan keys by
/// callee name; two same-named acceptors with different indices would make
/// the scan ambiguous).
fn acceptor_index_by_name(
    acceptors: &std::collections::BTreeMap<(String, String), (usize, String)>,
) -> std::collections::BTreeMap<String, (usize, String)> {
    let mut by_name = std::collections::BTreeMap::new();
    for ((module, fn_name), (idx, param)) in acceptors {
        if let Some((prev_idx, _)) = by_name.insert(fn_name.clone(), (*idx, param.clone())) {
            assert_eq!(
                prev_idx, *idx,
                "surface-accepting fns named `{fn_name}` (latest in {module}) \
                 disagree on the surface argument index — the position scan \
                 would be ambiguous; rename one"
            );
        }
    }
    by_name
}

fn render_position_rows(
    rows: &std::collections::BTreeMap<(String, String, String, String), usize>,
) -> String {
    rows.iter()
        .map(|((m, f, c, b), n)| format!("    (\"{m}\", \"{f}\", \"{c}\", \"{b}\", {n}),\n"))
        .collect()
}

#[test]
fn surface_accepting_fns_are_the_closed_pinned_inventory() {
    let discovered = surface_accepting_fns();
    let pinned: std::collections::BTreeMap<(String, String), (usize, String)> =
        SURFACE_ACCEPTING_FNS
            .iter()
            .map(|(m, f, idx, p)| ((m.to_string(), f.to_string()), (*idx, p.to_string())))
            .collect();
    let rendered: String = discovered
        .iter()
        .map(|((m, f), (idx, p))| format!("    (\"{m}\", \"{f}\", {idx}, \"{p}\"),\n"))
        .collect();
    assert_eq!(
        discovered, pinned,
        "the surface-accepting fn inventory drifted — a new forwarding layer \
         (or a signature change) is a conscious vocabulary change, reviewed \
         here. Discovered:\n{rendered}"
    );
    acceptor_index_by_name(&discovered);
}

#[test]
fn every_authored_position_is_bound_to_its_pinned_surface() {
    let acceptors = surface_accepting_fns();
    let by_name = acceptor_index_by_name(&acceptors);
    assert!(
        by_name.contains_key("prepare_template_value"),
        "the preparation entry must be discovered as a surface acceptor"
    );
    let mut discovered: std::collections::BTreeMap<(String, String, String, String), usize> =
        std::collections::BTreeMap::new();
    for module in runtime_modules() {
        let file = parse_runtime_module(&module);
        let module_forwarders: std::collections::BTreeMap<String, String> = acceptors
            .iter()
            .filter(|((m, _), _)| *m == module)
            .map(|((_, f), (_, p))| (f.clone(), p.clone()))
            .collect();
        discovered.extend(authored_position_sites(
            &module,
            &file,
            &by_name,
            &module_forwarders,
        ));
    }
    // Typo-proofing: every pinned LITERAL binding names a real surface
    // variant.
    let owner = parse_runtime_module(OWNER);
    let variants: BTreeSet<String> = find_enum(&owner, "AuthoredValueSurface")
        .expect("AuthoredValueSurface enum")
        .variants
        .iter()
        .map(|v| v.ident.to_string())
        .collect();
    for (_, _, _, binding, _) in AUTHORED_POSITION_SURFACE_BINDINGS {
        if !binding.starts_with("forwarded(") {
            assert!(
                variants.contains(*binding),
                "pinned binding `{binding}` is not an AuthoredValueSurface variant"
            );
        }
    }
    let pinned: std::collections::BTreeMap<(String, String, String, String), usize> =
        AUTHORED_POSITION_SURFACE_BINDINGS
            .iter()
            .map(|(m, f, c, b, n)| {
                (
                    (m.to_string(), f.to_string(), c.to_string(), b.to_string()),
                    *n,
                )
            })
            .collect();
    assert_eq!(
        pinned.len(),
        AUTHORED_POSITION_SURFACE_BINDINGS.len(),
        "duplicate rows in AUTHORED_POSITION_SURFACE_BINDINGS"
    );
    if discovered != pinned {
        let unlisted: std::collections::BTreeMap<_, _> = discovered
            .iter()
            .filter(|(k, n)| pinned.get(*k) != Some(*n))
            .map(|(k, n)| (k.clone(), *n))
            .collect();
        let stale: std::collections::BTreeMap<_, _> = pinned
            .iter()
            .filter(|(k, n)| discovered.get(*k) != Some(*n))
            .map(|(k, n)| (k.clone(), *n))
            .collect();
        panic!(
            "every authored position must be bound to its pinned surface in \
             AUTHORED_POSITION_SURFACE_BINDINGS — a new position, surface \
             swap, laundered local, or count drift is classified here, \
             consciously.\nUNLISTED/CHANGED (discovered, not pinned):\n{}\
             STALE (pinned, not discovered):\n{}",
            render_position_rows(&unlisted),
            render_position_rows(&stale),
        );
    }
    // Non-vacuity: the preparation entry is called from many authored
    // positions; the forwarding lanes exist.
    let prepare_calls: usize = discovered
        .iter()
        .filter(|((_, _, c, _), _)| c == "prepare_template_value")
        .map(|(_, n)| *n)
        .sum();
    assert!(
        prepare_calls >= 30,
        "the preparation-entry position inventory must stay non-vacuous \
         (found {prepare_calls} calls)"
    );
    assert!(
        discovered
            .keys()
            .any(|(_, _, _, b)| b.starts_with("forwarded(")),
        "the forwarding lanes must stay visible to the scan"
    );
}

// ── position-binding discrimination self-tests ───────────────────────────────

#[test]
fn position_scan_discriminates_a_planted_raw_surface_position() {
    // Mutation (i): a NEW authored position selecting an existing
    // Raw-classified surface (`EventHandler`) for a should-wrap value. The
    // scan discovers the site; the pinned inventory does not contain it — the
    // inventory-equality gate fails.
    let planted: syn::File = syn::parse_str(
        "impl<'a> SupportedClientIr<'a> {\n\
             fn sneaky_position(&self, expr: ExprId) -> Result<(), E> {\n\
                 let _ = self.prepare_template_value(\n\
                     AuthoredExpr(expr),\n\
                     AuthoredValueSurface::EventHandler,\n\
                 )?;\n\
                 Ok(())\n\
             }\n\
         }",
    )
    .expect("planted snippet parses");
    let by_name = acceptor_index_by_name(&surface_accepting_fns());
    let sites = authored_position_sites(
        "planted.rs",
        &planted,
        &by_name,
        &std::collections::BTreeMap::new(),
    );
    assert_eq!(
        sites.get(&(
            "planted.rs".to_string(),
            "sneaky_position".to_string(),
            "prepare_template_value".to_string(),
            "EventHandler".to_string(),
        )),
        Some(&1),
        "the position scan must discover a planted raw-surface position"
    );
    assert!(
        AUTHORED_POSITION_SURFACE_BINDINGS
            .iter()
            .all(|(_, f, _, _, _)| *f != "sneaky_position"),
        "a planted position must not be pre-classified"
    );
}

#[test]
fn position_scan_discriminates_a_surface_swap_and_a_laundered_local() {
    // A surface SWAP at an existing forwarding site: the forwarder passing a
    // literal Raw surface instead of forwarding its own parameter yields the
    // literal binding — never the pinned `forwarded(surface)` — so the
    // equality gate fails on the changed row.
    let swapped: syn::File = syn::parse_str(
        "impl<'a> SupportedClientIr<'a> {\n\
             fn attr_value_for(&self, el: &ElementIr, name: &str, surface: AuthoredValueSurface) -> R {\n\
                 self.prepare_template_value(AuthoredExpr(e), AuthoredValueSurface::EventHandler)\n\
             }\n\
         }",
    )
    .expect("swapped snippet parses");
    let by_name = acceptor_index_by_name(&surface_accepting_fns());
    let forwarders: std::collections::BTreeMap<String, String> =
        [("attr_value_for".to_string(), "surface".to_string())]
            .into_iter()
            .collect();
    let sites = authored_position_sites("swapped.rs", &swapped, &by_name, &forwarders);
    assert_eq!(
        sites.get(&(
            "swapped.rs".to_string(),
            "attr_value_for".to_string(),
            "prepare_template_value".to_string(),
            "EventHandler".to_string(),
        )),
        Some(&1),
        "a swapped literal surface must be described as the literal, not as forwarded"
    );
    assert!(
        !sites.keys().any(|(_, _, _, b)| b == "forwarded(surface)"),
        "the swap must not masquerade as the pinned forwarded binding"
    );
    // A LAUNDERED local (`let s = …; prepare(_, s)`) is fail-closed: the
    // binding renders unrecognized and can never match a pinned row.
    let laundered: syn::File = syn::parse_str(
        "impl<'a> SupportedClientIr<'a> {\n\
             fn sneaky(&self, e: ExprId) -> R {\n\
                 let s = AuthoredValueSurface::EventHandler;\n\
                 self.prepare_template_value(AuthoredExpr(e), s)\n\
             }\n\
         }",
    )
    .expect("laundered snippet parses");
    let sites = authored_position_sites(
        "laundered.rs",
        &laundered,
        &by_name,
        &std::collections::BTreeMap::new(),
    );
    let binding = sites
        .keys()
        .find(|(_, f, _, _)| f == "sneaky")
        .map(|(_, _, _, b)| b.clone())
        .expect("the laundered call must be discovered");
    assert!(
        binding.starts_with("unrecognized("),
        "a laundered local must render fail-closed as unrecognized (got `{binding}`)"
    );
}

#[test]
fn runtime_module_universe_is_recursive() {
    // Mutation (ii) backing rail: the scan universe is filesystem-discovered
    // over the COMPLETE recursive module graph — a planner under a nested
    // submodule dir joins every inventory automatically; there is no pinned
    // module list to evade.
    let modules = runtime_modules();
    for nested in [
        "css/match.rs",
        "css/analyze.rs",
        "expr_rewrite/mod.rs",
        "expr_rewrite/plan.rs",
        "expr_rewrite/plan_render.rs",
    ] {
        assert!(
            modules.iter().any(|m| m == nested),
            "the recursive universe must include the nested module {nested}"
        );
    }
    // Test-only exclusion is PROVEN per module from the `#[cfg(test)]` module
    // graph — never assumed from a filename. Each excluded module below is
    // excluded because its `mod` declaration is `#[cfg(test)]`-gated.
    let test_only = test_only_runtime_modules();
    for proven in [
        "client_tests.rs",
        "client_shapes_tests.rs",
        "runtime_tests.rs",
        "diff_oracle_tests.rs",
        "css/match_tests.rs",
        "css/analyze_tests.rs",
        "reactive_fold_tests.rs",
    ] {
        assert!(
            test_only.contains(proven),
            "{proven} must be proven `#[cfg(test)]`-declared by the module-graph walk"
        );
        assert!(
            !modules.iter().any(|m| m == proven),
            "the proven test-only module {proven} stays out of the scan universe"
        );
    }
    // Production modules are classified production by their declarations.
    let classified = classify_module_graph(&crate_root().join("src/svelte/runtime"));
    for production in [
        OWNER,
        PRINTER_DEFINER,
        "css/match.rs",
        "expr_rewrite/mod.rs",
    ] {
        assert_eq!(
            classified.get(production),
            Some(&false),
            "{production} must be classified PRODUCTION by the module-graph walk"
        );
    }
}

#[test]
fn test_only_classification_is_by_cfg_test_declaration_not_filename() {
    // The classifier's discriminating core: an UNCONDITIONALLY-declared module
    // named `*_tests.rs` is PRODUCTION (it joins every rail); a
    // `#[cfg(test)]`-declared module is test-only regardless of its name; a
    // `#[path]`-declared test sibling resolves to its real file; a
    // `#[cfg(test)]` INLINE mod gates every nested declaration.
    let planted: syn::File = syn::parse_str(
        "mod rogue_tests;\n\
         #[cfg(test)]\n\
         mod real_tests;\n\
         #[cfg(test)]\n\
         #[path = \"sibling_tests.rs\"]\n\
         mod tests;\n\
         #[cfg(test)]\n\
         mod harness { mod nested_helper; }\n",
    )
    .expect("planted snippet parses");
    let mut decls = Vec::new();
    collect_out_of_line_mods(
        &planted.items,
        false,
        true,
        std::path::Path::new(""),
        &mut decls,
    );
    let rows: Vec<(Vec<String>, bool)> = decls
        .iter()
        .map(|d| {
            (
                d.candidates
                    .iter()
                    .map(|c| c.to_string_lossy().replace('\\', "/"))
                    .collect(),
                d.test_only,
            )
        })
        .collect();
    assert_eq!(
        rows,
        vec![
            (
                vec![
                    "rogue_tests.rs".to_string(),
                    "rogue_tests/mod.rs".to_string()
                ],
                false,
            ),
            (
                vec!["real_tests.rs".to_string(), "real_tests/mod.rs".to_string()],
                true,
            ),
            (vec!["sibling_tests.rs".to_string()], true),
            (
                vec![
                    "harness/nested_helper.rs".to_string(),
                    "harness/nested_helper/mod.rs".to_string(),
                ],
                true,
            ),
        ],
        "classification must come from the `#[cfg(test)]` declaration chain: \
         an unconditional `mod rogue_tests;` is PRODUCTION despite its \
         filename; cfg(test)-declared / cfg(test)-nested modules are test-only"
    );
}

#[test]
fn compound_and_negated_cfg_declarations_classify_production() {
    // CONSERVATIVE cfg classification: only the EXACT `#[cfg(test)]` predicate
    // is test-only. Every compound/negated/feature predicate can compile in a
    // production configuration, so the declared module stays in the scan
    // universe — including predicates that merely MENTION `test`
    // (`not(test)`, `any(test, …)`, `all(test, …)`) and feature names
    // containing the substring. A production-only planner behind
    // `#[cfg(not(test))]` must not evade the rails.
    let planted: syn::File = syn::parse_str(
        "#[cfg(not(test))]\n\
         mod negated;\n\
         #[cfg(any(test, feature = \"x\"))]\n\
         mod compound_any;\n\
         #[cfg(all(test, feature = \"x\"))]\n\
         mod compound_all;\n\
         #[cfg(feature = \"test-utils\")]\n\
         mod feature_named_test;\n",
    )
    .expect("planted snippet parses");
    let mut decls = Vec::new();
    collect_out_of_line_mods(
        &planted.items,
        false,
        true,
        std::path::Path::new(""),
        &mut decls,
    );
    let test_only: Vec<bool> = decls.iter().map(|d| d.test_only).collect();
    assert_eq!(
        test_only,
        vec![false, false, false, false],
        "every non-exact cfg predicate (negated/compound/feature) classifies \
         PRODUCTION: the declared module joins every scan rail"
    );
    // The fn/impl inventories apply the same rule: a fn inside a
    // `#[cfg(not(test))]` inline mod is a PRODUCTION fn (scanned); one inside
    // an exact `#[cfg(test)]` inline mod is not.
    let inline: syn::File = syn::parse_str(
        "#[cfg(not(test))]\n\
         mod negated {\n\
             fn scanned() {}\n\
         }\n\
         #[cfg(test)]\n\
         mod gated {\n\
             fn skipped() {}\n\
         }\n",
    )
    .expect("inline snippet parses");
    let mut fns = Vec::new();
    collect_production_fns(&inline.items, &mut fns);
    let names: Vec<&str> = fns.iter().map(|(n, _, _)| n.as_str()).collect();
    assert_eq!(
        names,
        vec!["scanned"],
        "the production-fn inventory scans `#[cfg(not(test))]` inline mods \
         and skips exact `#[cfg(test)]` ones"
    );
    let mut impl_fns = Vec::new();
    collect_production_fns_with_impl(&inline.items, &mut impl_fns);
    let impl_names: Vec<&str> = impl_fns.iter().map(|(_, n, _)| n.as_str()).collect();
    assert_eq!(
        impl_names,
        vec!["scanned"],
        "the impl-aware production-fn inventory applies the same rule"
    );
}

#[test]
fn exact_cfg_test_declaration_stays_test_only() {
    // The exact single bare-`test` predicate — and ONLY it — gates a module
    // out of the scan rails; other attributes alongside it do not defeat the
    // structural match.
    let planted: syn::File = syn::parse_str(
        "#[cfg(test)]\n\
         mod unit_tests;\n\
         #[allow(dead_code)]\n\
         #[cfg(test)]\n\
         mod attr_mixed_tests;\n",
    )
    .expect("planted snippet parses");
    let mut decls = Vec::new();
    collect_out_of_line_mods(
        &planted.items,
        false,
        true,
        std::path::Path::new(""),
        &mut decls,
    );
    let test_only: Vec<bool> = decls.iter().map(|d| d.test_only).collect();
    assert_eq!(
        test_only,
        vec![true, true],
        "an exact `#[cfg(test)]` declaration is test-only regardless of \
         sibling attributes"
    );
}

// ─────────── SECONDARY: retired-name / wrap-syntax tripwires ────────────────
//
// Substring scans over comment-stripped source. TRIPWIRES ONLY: they catch a
// reintroduction by exact spelling (a retired entry-point name, the wrap
// syntax bytes constructed outside the owner) and establish nothing else —
// the structural rails above are the enforcement mechanism for the
// inventoried topology (transitional — see the module header and D-61).

/// Retired entry points / carrier shapes that must never reappear anywhere in
/// the runtime backend.
const RETIRED_NAME_TOKENS: &[&str] = &[
    "legacy_wrap_value(",
    "legacy_prepared_value(",
    "prepare_render_callee_slice(",
    "legacy_wrapped:",
];

fn retired_tokens_in(stripped: &str) -> Vec<&'static str> {
    RETIRED_NAME_TOKENS
        .iter()
        .copied()
        .filter(|t| stripped.contains(t))
        .collect()
}

#[test]
fn retired_names_do_not_reappear_in_any_runtime_module() {
    for module in runtime_modules() {
        let stripped = strip_rust_comments(&read_runtime_source(&module));
        let hits = retired_tokens_in(&stripped);
        assert!(
            hits.is_empty(),
            "{module} reintroduces retired name(s) {hits:?}"
        );
    }
}

#[test]
fn wrap_syntax_construction_stays_in_the_owner() {
    // The legacy value-wrap SEQUENCE syntax (`$.untrack(` tail) is constructed
    // only inside the owner. `$.deep_read_state(` is additionally legitimate
    // in `client_plan_script.rs` — the `$:` reactive-statement dependency
    // thunk (a DIFFERENT official feature: `$.legacy_pre_effect` deps), not
    // the template value wrap.
    for module in runtime_modules() {
        let stripped = strip_rust_comments(&read_runtime_source(&module));
        if module != OWNER {
            assert!(
                !stripped.contains("$.untrack("),
                "{module} must not construct the legacy wrap `$.untrack(` syntax \
                 (owner-only)"
            );
        }
        if module != OWNER && module != "client_plan_script.rs" {
            assert!(
                !stripped.contains("$.deep_read_state("),
                "{module} must not construct `$.deep_read_state(` reads \
                 (owner + the `$:` deps thunk only)"
            );
        }
    }
}

// ── discrimination self-tests (the scanners catch planted violations) ───────

#[test]
fn ast_scan_discriminates_a_planted_wildcard_arm() {
    let planted: syn::File = syn::parse_str(
        "pub(super) const fn policy(surface: AuthoredValueSurface) -> ValuePolicy {\n\
             match surface {\n\
                 S::ReactiveText => raw(),\n\
                 _ => raw(),\n\
             }\n\
         }",
    )
    .expect("planted snippet parses");
    let (variants, non_path) = match_arm_variants(&planted, "policy");
    assert!(non_path, "the arm scanner must flag a planted wildcard arm");
    assert_eq!(variants, vec!["ReactiveText".to_string()]);
}

#[test]
fn ast_scan_discriminates_a_planted_consumer_and_ignores_comments() {
    let planted: syn::File =
        syn::parse_str("fn sneaky(expr: ExprId) { let _ = AuthoredExpr(expr); }")
            .expect("planted snippet parses");
    assert!(
        has_ident(&planted, "AuthoredExpr"),
        "the ident scan must find a planted AuthoredExpr consumer"
    );
    let commented: syn::File = syn::parse_str(
        "// AuthoredExpr( is only a comment mention\nfn clean() { let s = \"AuthoredExpr(\"; let _ = s; }",
    )
    .expect("commented snippet parses");
    assert!(
        !has_ident(&commented, "AuthoredExpr"),
        "a comment or string-literal mention is NOT an AST reference"
    );
}

#[test]
fn ast_scan_discriminates_a_planted_republicized_wrap_carrier() {
    let planted: syn::File = syn::parse_str(
        "pub(super) enum PreparedExpression { Raw(String), LegacySequence(String) }",
    )
    .expect("planted snippet parses");
    let e = find_enum(&planted, "PreparedExpression").expect("planted enum");
    assert!(
        !matches!(e.vis, syn::Visibility::Inherited),
        "the visibility check must flag a re-publicized wrap carrier"
    );
}

#[test]
fn tripwire_discriminates_planted_wrap_syntax_and_retired_names() {
    // A planted out-of-owner wrap construction is caught after comment
    // stripping…
    let planted =
        r#"fn sneaky(obj: &S) -> String { format!("$.untrack(() => ({}))", obj.raw_text()) }"#;
    assert!(
        strip_rust_comments(planted).contains("$.untrack("),
        "the wrap-syntax tripwire must see a planted construction"
    );
    // …while a comment-only mention strips away.
    assert!(
        !strip_rust_comments("// $.untrack( appears in prose only\nfn ok() {}")
            .contains("$.untrack("),
        "a comment-only mention must not trip the scan"
    );
    assert_eq!(
        retired_tokens_in(&strip_rust_comments(
            "fn sneaky(&self) { let w = self.legacy_wrap_value(expr, &rewritten); }"
        )),
        vec!["legacy_wrap_value("],
        "the retired-name tripwire must catch a planted retired-entry call"
    );
    assert!(
        retired_tokens_in(&strip_rust_comments(
            "// legacy_wrap_value( is only mentioned in a comment\nfn clean() {}"
        ))
        .is_empty(),
        "a comment-only retired-name mention must not trip the scan"
    );
}
