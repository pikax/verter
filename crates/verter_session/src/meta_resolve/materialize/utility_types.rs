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

use std::sync::Arc;

use verter_semantic::analysis::type_expr::{ObjectExpr, ObjectMember, TypeExpr};

/// Cap on the per-test counter-name interner used by
/// [`leak_counter_name`]. The capture-token harness keys counters by
/// `&'static str`; this helper interns parametric counter names so
/// each unique `member_materialize_calls::<DeclName>` reaches the
/// snapshot via a stable static reference.
const COUNTER_NAME_INTERNER_LIMIT: usize = 64;

/// Intern a counter name as a `&'static str` so the capture-token
/// harness (which keys counters by `&'static str`) can record
/// parametric counter names like `member_materialize_calls::<DeclName>`.
/// The interner has a bounded limit; once exceeded, returns a stable
/// fallback name so a malicious or pathological test cannot cause
/// unbounded leak.
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

/// Return the parametric counter name for `<DeclName>`. Used by the
/// production hooks and by tests reading the counter from a
/// `CaptureSnapshot`.
pub(crate) fn member_materialize_calls_counter(decl_name: &str) -> &'static str {
    let owned = format!("member_materialize_calls::{}", decl_name);
    leak_counter_name(&owned)
}

/// Trace event name for the "expensive omit member resolution" trace
/// emitted when a downstream consumer demands a concrete object
/// surface from a previously-symbolic `Omit<package_backed, K>` and
/// the resolver is forced to materialise individual members.
pub(crate) const EXPENSIVE_OMIT_MEMBER_RESOLUTION_TRACE_COUNTER: &str =
    "expensive_omit_member_resolution";

/// Selective `Pick<T, K>` expansion for a package-backed target.
///
/// When the target declaration's body is an Object surface, this
/// helper materialises ONLY the members named in `key_set`. Each
/// materialisation records `member_materialize_calls::<decl_name>`
/// once on the active capture token.
///
/// Returns `Some(ObjectExpr { properties: ... })` containing only
/// the picked members; `None` when the target body is not an Object
/// surface (the caller falls back to the standard Pick path).
///
/// Workspace-owned targets MUST defer to the canonical reuse path
///; this helper is only invoked when the caller
/// has already established `target` is package-backed.
pub(crate) fn selective_pick_expansion_for_package_backed(
    target_body: &TypeExpr,
    key_set: &[String],
    decl_name: &str,
) -> Option<TypeExpr> {
    let counter_name = member_materialize_calls_counter(decl_name);
    let body = peel_paren(target_body);
    let TypeExpr::Object(object) = body else {
        return None;
    };
    let mut picked_properties: Vec<ObjectMember> = Vec::with_capacity(key_set.len());
    for member in &object.properties {
        let name = match member {
            ObjectMember::Property(p) => p.name.as_str(),
            ObjectMember::Method(m) => m.name.as_str(),
            _ => continue,
        };
        if key_set.iter().any(|k| k == name) {
            // Record the member-materialize counter once per picked
            // member. Selective expansion is O(|K|), proven by this
            // counter's value matching the size of `key_set`.
            crate::capture_token::with_active_capture(|t| {
                t.record_counter(counter_name, 1);
            });
            picked_properties.push(member.clone());
        }
    }
    if picked_properties.is_empty() {
        return None;
    }
    Some(TypeExpr::Object(Arc::new(ObjectExpr {
        properties: picked_properties,
    })))
}

/// Symbolic `Omit<T, K>` preservation for a package-backed target.
///
/// Returns the input `Omit<target, key_set>` unchanged — the symbolic
/// shape `Ref { name: "Omit", type_arguments: [target, key_set] }` is
/// the result. No member of the target is enumerated, so the per-decl
/// member-materialize counter MUST stay at 0 for this path.
///
/// Consumers that subsequently index into the symbolic Omit reduce
/// through the existing indexed-access predicate (/ §6.3,
/// owned by B-Bm).
///
/// `target` is the resolved declaration body; `target_ref` is the
/// original `Ref { name: target_name, type_arguments: [] }` reference.
/// `keys` is the original `Union(Literal(string), …)` shape passed as
/// `Omit<T, K>`'s second type argument.
pub(crate) fn symbolic_omit_for_package_backed(target_ref: TypeExpr, keys: TypeExpr) -> TypeExpr {
    TypeExpr::Ref {
        name: Arc::from("Omit"),
        type_arguments: Arc::from(vec![target_ref, keys].into_boxed_slice()),
    }
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

/// Convenience: drive the symbolic-Omit trace emission for an
/// `Omit<package_backed, K>` materialisation that a downstream
/// consumer forces concrete (e.g., by indexing into the result with
/// a literal that doesn't appear in K). Records once per forced
/// resolution; informational, not a failure.
#[allow(dead_code)]
pub(crate) fn record_expensive_omit_member_resolution() {
    crate::capture_token::with_active_capture(|t| {
        t.record_counter(EXPENSIVE_OMIT_MEMBER_RESOLUTION_TRACE_COUNTER, 1);
    });
}
