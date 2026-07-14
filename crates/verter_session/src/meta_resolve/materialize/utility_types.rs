//! Selective `Pick<package_backed, K>` and symbolic
//! `Omit<package_backed, K>` materialisation helpers.
//!
//! When the target declaration of a `Pick<T, K>` or `Omit<T, K>`
//! utility resolves to a package-backed source (i.e., the canonical
//! id resolves under `/node_modules/`), the materialiser does NOT
//! enumerate the target's full member surface. Instead:
//!
//! - **Selective Pick**: only members whose name appears in the key
//!   set traverse the materialisation path. Each materialised member
//!   records `member_materialize_calls::<DeclName>` once via the
//!   per-request capture token. Cost is O(|K|), not O(|target.body|).
//! - **Symbolic Omit**: the resulting `TypeExpr` stays as
//!   `Ref { name: "Omit", type_arguments: [target, key_set] }` —
//!   no member of the target is enumerated, so
//!   `member_materialize_calls::<DeclName>` remains 0. Consumers that
//!   index into the symbolic Omit reduce through the existing
//!   indexed-access predicate without forcing concrete enumeration.
//!
//! For workspace-owned targets, both helpers decline so the canonical
//! reuse path keeps ownership of the materialisation
//! result.
//!
//! ## Counter contract
//!
//! `member_materialize_calls::<DeclName>` (capture-token counter) is
//! recorded ONLY by the selective-Pick path, once per materialised
//! member. The symbolic-Omit path does not increment it. Tests in
//! `crates/verter_session/src/component_meta_pick_omit_tests.rs`
//! discriminate post-fix behaviour via this counter.

use verter_type_expr::TypeExpr;

/// Cap on the per-test counter-name interner used by
/// [`leak_counter_name`]. The capture-token harness keys counters by
/// `&'static str`; this helper interns parametric counter names so
/// each unique `member_materialize_calls::<DeclName>` reaches the
/// snapshot via a stable static reference.
#[cfg(any(test, debug_assertions))]
const COUNTER_NAME_INTERNER_LIMIT: usize = 64;

/// Intern a counter name as a `&'static str` so the capture-token
/// harness (which keys counters by `&'static str`) can record
/// parametric counter names like `member_materialize_calls::<DeclName>`.
/// The interner has a bounded limit; once exceeded, returns a stable
/// fallback name so a malicious or pathological test cannot cause
/// unbounded leak.
///
/// Test/debug instrumentation only — gated to match the capture-token
/// module (absent in release).
#[cfg(any(test, debug_assertions))]
pub(crate) fn leak_counter_name(name: &str) -> &'static str {
    use parking_lot::Mutex;
    use rustc_hash::FxHashMap;
    use std::sync::OnceLock;
    static INTERNER: OnceLock<Mutex<FxHashMap<String, &'static str>>> = OnceLock::new();
    let interner = INTERNER.get_or_init(|| Mutex::new(FxHashMap::default()));
    let mut guard = interner.lock();
    if let Some(&existing) = guard.get(name) {
        return existing;
    }
    if guard.len() >= COUNTER_NAME_INTERNER_LIMIT {
        return "member_materialize_calls::__overflow__";
    }
    let leaked: &'static str = Box::leak(name.to_string().into_boxed_str());
    guard.insert(name.to_string(), leaked);
    leaked
}

/// Return the parametric counter name for `<DeclName>`. Test/debug
/// instrumentation only (the capture-token recording site and the tests
/// reading the counter from a `CaptureSnapshot`); gated to match the
/// capture-token module (absent in release).
#[cfg(any(test, debug_assertions))]
pub(crate) fn member_materialize_calls_counter(decl_name: &str) -> &'static str {
    let owned = format!("member_materialize_calls::{}", decl_name);
    leak_counter_name(&owned)
}

/// Predicate: is `body` a "non-object" (mapped/conditional/etc.)
/// shape that the symbolic-Omit path should NOT bypass? When true,
/// the caller falls through to the standard pick/omit path.
#[allow(dead_code)]
pub(crate) fn body_is_non_object_helper_alias(body: &TypeExpr) -> bool {
    !matches!(peel_paren(body), TypeExpr::Object(_) | TypeExpr::Ref { .. })
}

/// Strip leading `Parenthesized` wrappers.
fn peel_paren(expr: &TypeExpr) -> &TypeExpr {
    match expr {
        TypeExpr::Parenthesized(inner) => peel_paren(inner),
        _ => expr,
    }
}
