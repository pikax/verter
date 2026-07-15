//! R6/R21 structural guards for four query-identity cache keys:
//!
//! - [`ComponentMetaResultKey`] — final component-meta result.
//! - `RouteNameKey` / `BarrelSurfaceKey` — RouteDb per-name + barrel
//!   surface.
//! - `RefCycleResultKey` — transitive-cycle BFS result.
//! - `MaterializationCacheKey` — structural materialiser.
//!
//! Each is a **query-identity** cache key (R5): its slot identity is
//! content-free, and per-value version rooting lives on the cached value
//! (`ReadSetSignature.facts` + `self_root_canonicals` /
//! `validated_at_generation`), never in the key. R6 forbids any
//! content/version hash on the key; R21 requires the split env-hash
//! dimensions the value depends on to be PRESENT on the key (so concurrent
//! env/project variants never alias onto one slot).
//!
//! Two-layer enforcement per key:
//!
//! 1. **Source brace-block scan** — locate the key's `pub struct … {`
//!    body and assert (a) it contains NONE of the forbidden
//!    content/version field markers (R6) nor the bundled
//!    `project_config_hash` (R21), and (b) it contains every required
//!    split-env axis name (R21). The required-axis arm is the
//!    discriminating RED→GREEN check: against the pre-migration key
//!    (which omits the env axes) it FAILS; against the migrated key it
//!    PASSES.
//! 2. **Compile-time destructuring + runtime env-discrimination** — the
//!    exhaustive `let Key { … } = key;` destructure fails to compile if a
//!    forbidden field is added or a required axis removed; the runtime
//!    tests construct two keys differing in exactly one env axis and
//!    assert distinct hashes (no cross-env aliasing).
//!
//! The shared predicate [`key_shape_violations`] has its own discriminator
//! self-test ([`key_shape_predicate_discriminates`]) so the guard is not a
//! stub — it provably trips on a missing axis or a re-introduced
//! content/version field and provably accepts a compliant body.

use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::Arc;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("repo root")
        .to_path_buf()
}

fn read_file(rel: &str) -> String {
    let path = workspace_root().join(rel);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read `{rel}`: {e}"))
}

/// Locate the first `{ ... }` brace block following a textual `needle` in
/// `source`. Returns the inner text between the matching `{` and the
/// corresponding `}` (excluding the braces), or `None` if the needle is
/// missing or the braces are unbalanced.
fn extract_brace_block(source: &str, needle: &str) -> Option<String> {
    let start = source.find(needle)?;
    let bytes = source.as_bytes();
    let mut depth = 0usize;
    let mut open: Option<usize> = None;
    for i in start..bytes.len() {
        match bytes[i] {
            b'{' => {
                if depth == 0 {
                    open = Some(i);
                }
                depth += 1;
            }
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    let begin = open? + 1;
                    return Some(source[begin..i].to_string());
                }
            }
            _ => {}
        }
    }
    None
}

/// Field-shape markers that, present in a query-identity key body, would
/// re-introduce a content/version hash (R6) or a bundled env hash (R21).
const FORBIDDEN_KEY_MARKERS: &[&str] = &[
    "whole_hash:",
    "content_hash:",
    "parse_stable_hash:",
    "fact_dep_signature:",
    // R21: the five env dims must stay split — a single bundled
    // `project_config_hash` is forbidden.
    "project_config_hash",
];

/// Compute the R6/R21 violations for a key struct body.
///
/// - For each marker in `forbidden`, a PRESENT marker is a violation
///   (R6: no content/version hash; R21: no bundled hash).
/// - For each axis in `required_present`, an ABSENT axis is a violation
///   (R21: the split env dim the value depends on must key the slot).
///
/// Returns the list of human-readable violations; empty == compliant.
fn key_shape_violations(body: &str, required_present: &[&str], forbidden: &[&str]) -> Vec<String> {
    let mut violations = Vec::new();
    for needle in forbidden {
        if body.contains(needle) {
            violations.push(format!(
                "FORBIDDEN field marker `{needle}` present (R6/R21)"
            ));
        }
    }
    for axis in required_present {
        if !body.contains(axis) {
            violations.push(format!("REQUIRED env axis `{axis}` absent (R21)"));
        }
    }
    violations
}

fn hash_of<T: Hash>(value: &T) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

/// The shared predicate must provably trip on a missing required axis AND
/// on a re-introduced forbidden field, and provably accept a compliant
/// body. Without this, the source-scan guards below would be stubs.
#[test]
fn key_shape_predicate_discriminates() {
    let required = &["project_identity", "resolve_env_hash"];
    let forbidden = FORBIDDEN_KEY_MARKERS;

    // Compliant body — no forbidden markers, both axes present.
    let ok = "pub owner_canonical: Arc<str>, pub project_identity: ProjectIdentity, \
              pub resolve_env_hash: Hash16,";
    assert!(
        key_shape_violations(ok, required, forbidden).is_empty(),
        "predicate must accept a compliant body",
    );

    // Missing a required axis.
    let missing_axis = "pub owner_canonical: Arc<str>, pub project_identity: ProjectIdentity,";
    let v = key_shape_violations(missing_axis, required, forbidden);
    assert!(
        v.iter().any(|m| m.contains("resolve_env_hash")),
        "predicate must flag a missing required env axis; got {v:?}",
    );

    // Re-introduced content/version field.
    let has_whole_hash =
        "pub project_identity: ProjectIdentity, pub resolve_env_hash: Hash16, pub whole_hash: Hash16,";
    let v = key_shape_violations(has_whole_hash, required, forbidden);
    assert!(
        v.iter().any(|m| m.contains("whole_hash:")),
        "predicate must flag a re-introduced content/version field; got {v:?}",
    );

    // Bundled env hash (R21 ban).
    let bundled = "pub owner_canonical: Arc<str>, pub project_config_hash: Hash16,";
    let v = key_shape_violations(bundled, required, forbidden);
    assert!(
        v.iter().any(|m| m.contains("project_config_hash")),
        "predicate must flag a bundled project_config_hash; got {v:?}",
    );
}

// ---------------------------------------------------------------------------
// ComponentMetaResultKey
// ---------------------------------------------------------------------------

/// R6/R21 source guard: the `ComponentMetaResultKey` struct body must carry
/// the split env axes (`project_identity`, `parse_env_hash`,
/// `resolve_env_hash`, `type_env_hash`, `lib_env_hash`) and NONE of the
/// forbidden content/version markers. The owner whole-hash stays the
/// value-side candidate discriminant (`BoundedCandidateMap<…, Hash16, …>`),
/// never a key field.
#[test]
fn component_meta_result_key_carries_split_env_and_no_content_hash() {
    let source = read_file("crates/verter_session/src/component_meta_result_db.rs");
    let body = extract_brace_block(&source, "pub struct ComponentMetaResultKey {").expect(
        "R6/R21 GUARD: could not locate `pub struct ComponentMetaResultKey` body in \
         component_meta_result_db.rs",
    );
    let required = &[
        "project_identity",
        "parse_env_hash",
        "resolve_env_hash",
        "type_env_hash",
        "lib_env_hash",
    ];
    let violations = key_shape_violations(&body, required, FORBIDDEN_KEY_MARKERS);
    assert!(
        violations.is_empty(),
        "R6/R21 GUARD VIOLATION — `ComponentMetaResultKey` (a query-identity slot \
         key) must carry the split env axes per R21 and NO content/version hash \
         per R6 (the owner whole-hash is the value-side candidate discriminant, \
         not a key field). Violations:\n  {}\nBody:\n{}",
        violations.join("\n  "),
        body
    );
}

/// Compile-time + runtime lock: the migrated key destructures to exactly
/// the documented field set, and two keys differing in exactly one env
/// axis hash distinctly (no cross-env / cross-project aliasing). Identical
/// owner+options+env → identical slot (warm-hit IS allowed).
#[test]
fn component_meta_result_key_env_axes_discriminate() {
    use verter_session::component_meta_result_db::ComponentMetaResultKey;
    use verter_session::file_artifact_store::ProjectIdentity;

    let base = ComponentMetaResultKey {
        owner_canonical: Arc::from("/w/Accordion.vue"),
        options_fingerprint: [7u8; 16],
        project_identity: ProjectIdentity([1u8; 16]),
        parse_env_hash: [2u8; 16],
        resolve_env_hash: [3u8; 16],
        type_env_hash: [4u8; 16],
        lib_env_hash: [5u8; 16],
    };

    // Exhaustive destructure — fails to COMPILE if a forbidden content
    // field is added or a required env axis removed.
    let ComponentMetaResultKey {
        owner_canonical: _,
        options_fingerprint: _,
        project_identity: _,
        parse_env_hash: _,
        resolve_env_hash: _,
        type_env_hash: _,
        lib_env_hash: _,
    } = &base;

    // `project_identity` independently discriminates the slot.
    let mut with_project = base.clone();
    with_project.project_identity = ProjectIdentity([9u8; 16]);
    assert_ne!(
        hash_of(&base),
        hash_of(&with_project),
        "project_identity must distinguish the ComponentMetaResultKey slot",
    );

    // Each `*_env_hash` axis independently discriminates the slot.
    let mutators: [fn(&mut ComponentMetaResultKey); 4] = [
        |k| k.parse_env_hash = [99u8; 16],
        |k| k.resolve_env_hash = [99u8; 16],
        |k| k.type_env_hash = [99u8; 16],
        |k| k.lib_env_hash = [99u8; 16],
    ];
    for mutate in mutators {
        let mut variant = base.clone();
        mutate(&mut variant);
        assert_ne!(
            hash_of(&base),
            hash_of(&variant),
            "each split env axis must distinguish the ComponentMetaResultKey slot",
        );
    }

    // Identical env + owner + options → identical slot (warm-hit allowed).
    assert_eq!(hash_of(&base), hash_of(&base.clone()));
}

// ---------------------------------------------------------------------------
// RouteDb — RouteNameKey + BarrelSurfaceKey
// ---------------------------------------------------------------------------

/// R6/R21 source guard: the `RouteNameKey` struct body must carry the
/// split env axes a route resolution depends on (`project_identity`,
/// `resolve_env_hash`, `lib_env_hash` — module augmentations stitch into
/// the route surface, hence lib) plus the `symbol_space` discriminator and
/// the `resolver_version` substrate bump, and NONE of the forbidden
/// content/version markers. `parse_env_hash`/`type_env_hash` do NOT key a
/// route surface (R21 scoping — routes are resolve-domain).
#[test]
fn route_name_key_carries_split_env_and_no_content_hash() {
    let source = read_file("crates/verter_session/src/resolver_core/route_db.rs");
    let body = extract_brace_block(&source, "pub struct RouteNameKey {").expect(
        "R6/R21 GUARD: could not locate `pub struct RouteNameKey` body in route_db.rs — \
         the bare-string `(String, String)` route key must be replaced by a typed \
         env-bearing key",
    );
    let required = &[
        "symbol_space",
        "project_identity",
        "resolve_env_hash",
        "lib_env_hash",
        "resolver_version",
    ];
    let violations = key_shape_violations(&body, required, FORBIDDEN_KEY_MARKERS);
    assert!(
        violations.is_empty(),
        "R6/R21 GUARD VIOLATION — `RouteNameKey` must carry the split resolve/lib env \
         axes + symbol_space + resolver_version per R21 and NO content/version hash \
         per R6 (route freshness rides the value-side `ValidatedFactCache` fact \
         signature). Violations:\n  {}\nBody:\n{}",
        violations.join("\n  "),
        body
    );
}

/// R6/R21 source guard: the `BarrelSurfaceKey` struct body must carry the
/// split env axes (`project_identity`, `resolve_env_hash`, `lib_env_hash`)
/// + `resolver_version`, and NONE of the forbidden content/version markers.
#[test]
fn barrel_surface_key_carries_split_env_and_no_content_hash() {
    let source = read_file("crates/verter_session/src/resolver_core/route_db.rs");
    let body = extract_brace_block(&source, "pub struct BarrelSurfaceKey {").expect(
        "R6/R21 GUARD: could not locate `pub struct BarrelSurfaceKey` body in route_db.rs — \
         the bare-string barrel key must be replaced by a typed env-bearing key",
    );
    let required = &[
        "project_identity",
        "resolve_env_hash",
        "lib_env_hash",
        "resolver_version",
    ];
    let violations = key_shape_violations(&body, required, FORBIDDEN_KEY_MARKERS);
    assert!(
        violations.is_empty(),
        "R6/R21 GUARD VIOLATION — `BarrelSurfaceKey` must carry the split resolve/lib \
         env axes + resolver_version per R21 and NO content/version hash per R6. \
         Violations:\n  {}\nBody:\n{}",
        violations.join("\n  "),
        body
    );
}

/// R6/R21 storage-type source guard: the `RouteDb` per-name and barrel
/// caches must NOT regress to the bare-string `ValidatedFactCache<(String,
/// String), …>` / `ValidatedFactCache<String, BarrelRouteSurface>` keys
/// (which omit every env axis — an R21 violation that lets a route resolved
/// under project/env A satisfy a lookup under project/env B). They must be
/// keyed by the typed `RouteNameKey` / `BarrelSurfaceKey`.
#[test]
fn route_db_does_not_key_routes_or_barrels_on_bare_strings() {
    let source = read_file("crates/verter_session/src/resolver_core/route_db.rs");
    const FORBIDDEN_STORAGE: &[&str] = &[
        "ValidatedFactCache<(String, String), RouteResult>",
        "ValidatedFactCache<String, BarrelRouteSurface>",
        "SingleflightGroup<(String, String),",
        "SingleflightGroup<String, Arc<BarrelRouteSurface>",
    ];
    let mut offenders = Vec::new();
    for needle in FORBIDDEN_STORAGE {
        if source.contains(needle) {
            offenders.push(*needle);
        }
    }
    assert!(
        offenders.is_empty(),
        "R6/R21 GUARD VIOLATION — `RouteDb` keys a route/barrel cache or singleflight \
         on bare strings, omitting the split env axes (R21). Use the typed \
         `RouteNameKey` / `BarrelSurfaceKey`. Offending storage types:\n  {}",
        offenders.join("\n  ")
    );
}

/// Runtime lock: `RouteNameKey` / `BarrelSurfaceKey` discriminate on each
/// env axis (no cross-env / cross-project / cross-space aliasing), and an
/// exhaustive destructure pins the field set.
#[test]
fn route_keys_env_axes_discriminate() {
    use verter_semantic::facts::registry::SymbolSpace;
    use verter_session::file_artifact_store::ProjectIdentity;
    use verter_session::resolver_core::{BarrelSurfaceKey, RouteNameKey};

    let route = RouteNameKey {
        provider_canonical: Arc::from("/w/index.ts"),
        exported_name: Arc::from("Foo"),
        symbol_space: SymbolSpace::Type,
        project_identity: ProjectIdentity([1u8; 16]),
        resolve_env_hash: [2u8; 16],
        lib_env_hash: [3u8; 16],
        resolver_version: 7,
    };
    let RouteNameKey {
        provider_canonical: _,
        exported_name: _,
        symbol_space: _,
        project_identity: _,
        resolve_env_hash: _,
        lib_env_hash: _,
        resolver_version: _,
    } = &route;
    let route_mutators: [fn(&mut RouteNameKey); 4] = [
        |k| k.symbol_space = SymbolSpace::Value,
        |k| k.project_identity = ProjectIdentity([9u8; 16]),
        |k| k.resolve_env_hash = [9u8; 16],
        |k| k.lib_env_hash = [9u8; 16],
    ];
    for mutate in route_mutators {
        let mut variant = route.clone();
        mutate(&mut variant);
        assert_ne!(
            hash_of(&route),
            hash_of(&variant),
            "each split axis must distinguish the RouteNameKey slot",
        );
    }
    assert_eq!(hash_of(&route), hash_of(&route.clone()));

    let barrel = BarrelSurfaceKey {
        barrel_canonical: Arc::from("/w/barrel.ts"),
        project_identity: ProjectIdentity([1u8; 16]),
        resolve_env_hash: [2u8; 16],
        lib_env_hash: [3u8; 16],
        resolver_version: 7,
    };
    let BarrelSurfaceKey {
        barrel_canonical: _,
        project_identity: _,
        resolve_env_hash: _,
        lib_env_hash: _,
        resolver_version: _,
    } = &barrel;
    let barrel_mutators: [fn(&mut BarrelSurfaceKey); 3] = [
        |k| k.project_identity = ProjectIdentity([9u8; 16]),
        |k| k.resolve_env_hash = [9u8; 16],
        |k| k.lib_env_hash = [9u8; 16],
    ];
    for mutate in barrel_mutators {
        let mut variant = barrel.clone();
        mutate(&mut variant);
        assert_ne!(
            hash_of(&barrel),
            hash_of(&variant),
            "each split axis must distinguish the BarrelSurfaceKey slot",
        );
    }
    assert_eq!(hash_of(&barrel), hash_of(&barrel.clone()));
}

// ---------------------------------------------------------------------------
// RefCycleResultDb — RefCycleResultKey
// ---------------------------------------------------------------------------

/// R6 source guard: the `RefCycleResultKey` struct body must be a
/// content-free slot key — it roots on the env-bearing
/// `ResolvedDeclSlotIdentity` slot (which carries `type_env_hash` /
/// `lib_env_hash` / `project_identity`), carries the extra
/// `resolve_env_hash` + `version`, and must NOT embed the versioned
/// `DeclIdentity` (which carries `whole_hash`) nor any content/version
/// marker. Per-content-version rooting lives on the cached value's
/// `ReadSetSignature.facts` + `self_root_canonicals`.
#[test]
fn ref_cycle_result_key_is_content_free_slot_keyed() {
    let source = read_file("crates/verter_session/src/component_meta_caches.rs");
    let body = extract_brace_block(&source, "pub struct RefCycleResultKey {").expect(
        "R6 GUARD: could not locate `pub struct RefCycleResultKey` body in \
         component_meta_caches.rs — the versioned `DeclIdentity` key must be replaced \
         by a content-free slot key",
    );
    let required = &["ResolvedDeclSlotIdentity", "resolve_env_hash", "version"];
    // The versioned `DeclIdentity` (embeds `whole_hash`) is forbidden as an
    // embed; `ResolvedDeclSlotIdentity` does NOT contain the substring
    // `DeclIdentity`, so the check does not false-trip on the slot type.
    let forbidden: Vec<&str> = FORBIDDEN_KEY_MARKERS
        .iter()
        .copied()
        .chain(std::iter::once("DeclIdentity"))
        .collect();
    let violations = key_shape_violations(&body, required, &forbidden);
    assert!(
        violations.is_empty(),
        "R6 GUARD VIOLATION — `RefCycleResultKey` must be a content-free slot key \
         (root: ResolvedDeclSlotIdentity + resolve_env_hash + version), NOT a versioned \
         `DeclIdentity`. Per-version rooting lives on the cached value's self-roots. \
         Violations:\n  {}\nBody:\n{}",
        violations.join("\n  "),
        body
    );
}

/// R6 storage-type source guard: the `RefCycleResultDb` store + inflight
/// table must be keyed on the content-free `RefCycleResultKey`, NOT the
/// versioned `DeclIdentity` (which embeds `whole_hash` — a content/version
/// hash in the cache key, the R6 violation). The struct body must therefore
/// reference no `DeclIdentity`.
#[test]
fn ref_cycle_db_is_keyed_on_content_free_slot_not_decl_identity() {
    let source = read_file("crates/verter_session/src/component_meta_caches.rs");
    let body = extract_brace_block(&source, "pub struct RefCycleResultDb {").expect(
        "R6 GUARD: could not locate `pub struct RefCycleResultDb` body in \
         component_meta_caches.rs",
    );
    assert!(
        !body.contains("DeclIdentity"),
        "R6 GUARD VIOLATION — `RefCycleResultDb` keys its store/inflight on the \
         versioned `DeclIdentity` (which embeds `whole_hash`). A query-identity cache \
         key must be content-free — key on `RefCycleResultKey` and root the content \
         version on the cached value. Body:\n{}",
        body
    );
}

/// Runtime lock: `RefCycleResultKey` is content-free (two content versions
/// of the same root → the SAME key, so they co-locate as candidates in one
/// slot), and it discriminates on `resolve_env_hash` + `version` + the
/// slot's identity. An exhaustive destructure pins the field set (no
/// `whole_hash`, no embedded `DeclIdentity`).
#[test]
fn ref_cycle_result_key_is_content_free_and_env_discriminating() {
    use verter_session::component_meta_caches::{RefCycleResultKey, REF_CYCLE_RESULT_VERSION};
    use verter_session::semantic_query::ResolvedDeclSlotIdentity;

    // The slot builder takes NO content/version input — two content
    // versions of `/a.ts:Foo` yield the identical slot, hence the identical
    // key. That is the content-version co-location the migration delivers:
    // the whole-hash never enters the key, only the value's self-roots.
    let slot_v1 =
        ResolvedDeclSlotIdentity::type_slot_unscoped(Arc::from("/a.ts"), Arc::from("Foo"));
    let slot_v2 =
        ResolvedDeclSlotIdentity::type_slot_unscoped(Arc::from("/a.ts"), Arc::from("Foo"));
    let key_v1 = RefCycleResultKey {
        root: slot_v1,
        resolve_env_hash: [2u8; 16],
        version: REF_CYCLE_RESULT_VERSION,
    };
    let key_v2 = RefCycleResultKey {
        root: slot_v2,
        resolve_env_hash: [2u8; 16],
        version: REF_CYCLE_RESULT_VERSION,
    };
    assert_eq!(
        hash_of(&key_v1),
        hash_of(&key_v2),
        "content-free: two content versions of the same root must hash to the same \
         RefCycleResultKey slot (they co-locate as candidates)",
    );

    // Exhaustive destructure — no `whole_hash`, no `DeclIdentity` embed.
    let RefCycleResultKey {
        root: _,
        resolve_env_hash: _,
        version: _,
    } = &key_v1;

    // resolve_env_hash discriminates.
    let mut env_variant = key_v1.clone();
    env_variant.resolve_env_hash = [9u8; 16];
    assert_ne!(
        hash_of(&key_v1),
        hash_of(&env_variant),
        "resolve_env_hash must distinguish the RefCycleResultKey slot",
    );

    // version discriminates.
    let mut version_variant = key_v1.clone();
    version_variant.version = REF_CYCLE_RESULT_VERSION.wrapping_add(1);
    assert_ne!(
        hash_of(&key_v1),
        hash_of(&version_variant),
        "version must distinguish the RefCycleResultKey slot",
    );

    // A different root (different decl name) discriminates.
    let other = RefCycleResultKey {
        root: ResolvedDeclSlotIdentity::type_slot_unscoped(Arc::from("/a.ts"), Arc::from("Bar")),
        resolve_env_hash: [2u8; 16],
        version: REF_CYCLE_RESULT_VERSION,
    };
    assert_ne!(
        hash_of(&key_v1),
        hash_of(&other),
        "a different root declaration must distinguish the RefCycleResultKey slot",
    );
}

/// R21 breadth: the `RefCycleResultKey`'s SLOT-EMBEDDED env axes
/// (`project_identity` / `type_env_hash` / `lib_env_hash`, carried on the
/// `ResolvedDeclSlotIdentity` root) each independently discriminate the key,
/// and an IDENTICAL slot env co-locates (content-free equivalence). The
/// sibling `ref_cycle_result_key_is_content_free_and_env_discriminating`
/// builds its root via `type_slot_unscoped` (ZERO env) and varies only
/// `resolve_env_hash` / `version` / the root NAME — it never moves the slot's
/// embedded env dimensions. This pairs that gap (the slot env axes) the way
/// `materialization_cache_key_is_content_free_and_env_discriminating` already
/// covers the materialize key's slot.
///
/// This is the R21 scoping rule for a `lib`-BEARING query-identity cache: the
/// five env dimensions are orthogonal, so two `RefCycleResultKey`s that differ
/// in EXACTLY one slot-embedded dimension MUST hash distinctly. If the slot
/// dimensions collapsed into one bundled hash (the R21 violation), a key
/// resolved under project/env A would alias a lookup under project/env B.
///
/// Discriminates: if `project_identity` / `lib_env_hash` / `type_env_hash`
/// were dropped from `ResolvedDeclSlotIdentity`'s `Hash`/`Eq` (env-collapse),
/// the per-axis `assert_ne!`s flip — two cross-project / cross-lib /
/// cross-type-env keys would hash-collide and over-share one cache slot.
#[test]
fn ref_cycle_result_key_slot_env_axes_discriminate() {
    use verter_session::component_meta_caches::{RefCycleResultKey, REF_CYCLE_RESULT_VERSION};
    use verter_session::semantic_query::ResolvedDeclSlotIdentity;

    // `type_slot` carries the env-bearing, content-free subject identity:
    // (canonical, name, project_identity, type_env_hash, lib_env_hash).
    let slot = |project_identity: u32, type_env: [u8; 16], lib_env: [u8; 16]| {
        ResolvedDeclSlotIdentity::type_slot(
            Arc::from("/a.ts"),
            Arc::from("Foo"),
            project_identity,
            type_env,
            lib_env,
        )
    };

    let base = RefCycleResultKey {
        root: slot(1, [2u8; 16], [3u8; 16]),
        resolve_env_hash: [4u8; 16],
        version: REF_CYCLE_RESULT_VERSION,
    };

    // Content-free equivalence: an IDENTICAL slot env (same project / type-env /
    // lib-env / canonical / name) co-locates onto the same key.
    let same = RefCycleResultKey {
        root: slot(1, [2u8; 16], [3u8; 16]),
        resolve_env_hash: [4u8; 16],
        version: REF_CYCLE_RESULT_VERSION,
    };
    assert_eq!(
        hash_of(&base),
        hash_of(&same),
        "content-free equivalence: an identical slot env must hash to the same \
         RefCycleResultKey slot (candidates co-locate)",
    );

    // Each slot-embedded env axis independently discriminates.
    for (label, variant_slot) in [
        ("project_identity", slot(9, [2u8; 16], [3u8; 16])),
        ("type_env_hash", slot(1, [99u8; 16], [3u8; 16])),
        ("lib_env_hash", slot(1, [2u8; 16], [99u8; 16])),
    ] {
        let variant = RefCycleResultKey {
            root: variant_slot,
            resolve_env_hash: [4u8; 16],
            version: REF_CYCLE_RESULT_VERSION,
        };
        assert_ne!(
            hash_of(&base),
            hash_of(&variant),
            "the slot-embedded `{label}` axis MUST distinguish the RefCycleResultKey \
             slot (R21 — the lib/type/project env dimensions are orthogonal; a \
             collapse would alias cross-{label} keys)",
        );
    }
}

// ---------------------------------------------------------------------------
// MaterializeStructureDb — MaterializationCacheKey
// ---------------------------------------------------------------------------

/// R6/R21 source guard: the `MaterializationCacheKey` struct body must be a
/// content-free CANONICAL SUBJECT key. Its subject is the env-bearing
/// `ResolvedDeclSlotIdentity` slot (carrying `project_identity` /
/// `type_env_hash` / `lib_env_hash`), plus the extra `resolve_env_hash`
/// the materialiser depends on (R21 — not carried by the slot), plus the
/// policy axis (`scope_axis`), the `projection_mode`, and the typed
/// `projection_path`. The graph-instance `base: SemanticNodeId` (the old
/// `MaterializeStructureCacheKey` subject) is FORBIDDEN as the subject;
/// `SemanticNodeId` may appear ONLY as `normalized_type_args`
/// (instantiation args, exactly as the already-compliant
/// `SemanticQueryKey::Instantiate { args: Arc<[SemanticNodeId]> }` keys
/// them — the violation was a SemanticNodeId *subject*, never the args).
/// No content/version hash on the key — freshness rides the value-side
/// self-root signature.
#[test]
fn materialization_cache_key_is_content_free_subject_keyed() {
    let source = read_file("crates/verter_session/src/component_meta_materialize.rs");
    let body = extract_brace_block(&source, "pub struct MaterializationCacheKey {").expect(
        "R6/R21 GUARD: could not locate `pub struct MaterializationCacheKey` body in \
         component_meta_materialize.rs — the graph-instance `base: SemanticNodeId` key \
         must be replaced by a content-free canonical subject key",
    );
    let required = &[
        "ResolvedDeclSlotIdentity",
        "resolve_env_hash",
        "scope_axis",
        "projection_mode",
        "projection_path",
    ];
    // `base: SemanticNodeId` (the old graph-instance subject) is forbidden
    // as the subject. `Arc<[SemanticNodeId]>` type-args do NOT match this
    // marker, so the check does not false-trip on the compliant args field.
    let forbidden: Vec<&str> = FORBIDDEN_KEY_MARKERS
        .iter()
        .copied()
        .chain(std::iter::once("base: SemanticNodeId"))
        .collect();
    let violations = key_shape_violations(&body, required, &forbidden);
    assert!(
        violations.is_empty(),
        "R6/R21 GUARD VIOLATION — `MaterializationCacheKey` must be a content-free \
         canonical SUBJECT key (decl: ResolvedDeclSlotIdentity + resolve_env_hash + \
         scope_axis + projection_mode + projection_path), NOT a graph-instance \
         `base: SemanticNodeId`. Per-version rooting lives on the cached value's \
         self-roots. Violations:\n  {}\nBody:\n{}",
        violations.join("\n  "),
        body
    );
}

/// R6/R21 storage-type source guard: the `MaterializeStructureDb` store +
/// inflight table must be keyed on the content-free
/// `MaterializationCacheKey`, NOT the graph-instance recursion identity
/// `MaterializeRuntimeKey` (which carries `base: SemanticNodeId`). The
/// runtime key stays the per-thread recursion/depth identity; the cache
/// keys on the canonical subject. The DB struct body must therefore
/// reference `MaterializationCacheKey` and NOT key the store/inflight on
/// `MaterializeRuntimeKey`.
#[test]
fn materialize_structure_db_is_keyed_on_canonical_subject_not_runtime_key() {
    let source = read_file("crates/verter_session/src/component_meta_caches.rs");
    let body = extract_brace_block(&source, "pub struct MaterializeStructureDb {").expect(
        "R6/R21 GUARD: could not locate `pub struct MaterializeStructureDb` body in \
         component_meta_caches.rs",
    );
    assert!(
        body.contains("MaterializationCacheKey"),
        "R6/R21 GUARD VIOLATION — `MaterializeStructureDb` must key its store/inflight on \
         the content-free `MaterializationCacheKey`. Body:\n{}",
        body
    );
    assert!(
        !body.contains("MaterializeRuntimeKey") && !body.contains("MaterializeStructureCacheKey"),
        "R6/R21 GUARD VIOLATION — `MaterializeStructureDb` keys its store/inflight on the \
         graph-instance recursion key (`base: SemanticNodeId`). A query-identity cache key \
         must be content-free — key on `MaterializationCacheKey` and root the content \
         version on the cached value. Body:\n{}",
        body
    );
}

/// Runtime lock: `MaterializationCacheKey` is a content-free canonical
/// SUBJECT key. An exhaustive destructure pins the field set (no
/// `base: SemanticNodeId` subject, no content/version hash). Two keys
/// with the SAME canonical subject co-locate (cross-owner reuse —
/// the key has no consumer-scope dimension); the R21 env axes
/// (`resolve_env_hash`, and the slot's `project_identity` /
/// `type_env_hash` / `lib_env_hash`), the policy axis (`scope_axis`),
/// the `projection_mode`, and the typed `projection_path` each
/// discriminate (no over-share across distinct subjects/envs/projections).
#[test]
fn materialization_cache_key_is_content_free_and_env_discriminating() {
    use verter_semantic::facts::registry::SymbolSpace;
    use verter_session::component_meta_materialize::{
        MaterializationCacheKey, MaterializationScope,
    };
    use verter_session::file_artifact_store::ProjectIdentity;
    use verter_session::resolver_core::RouteDemand;
    use verter_session::semantic_query::{ProjectionMode, ResolvedDeclSlotIdentity};

    // `type_slot` carries the env-bearing, content-free subject identity:
    // (canonical, name, project_identity, type_env_hash, lib_env_hash).
    let slot = |project_identity: u32, type_env: [u8; 16], lib_env: [u8; 16]| {
        ResolvedDeclSlotIdentity::type_slot(
            Arc::from("/w/ChatMessageProps.ts"),
            Arc::from("ChatMessageProps"),
            project_identity,
            type_env,
            lib_env,
        )
    };

    let base = MaterializationCacheKey {
        decl: slot(1, [2u8; 16], [3u8; 16]),
        projection_path: RouteDemand::Whole,
        scope_axis: MaterializationScope::TopLevel,
        projection_mode: ProjectionMode::Navigate,
        normalized_type_args: Arc::from(Vec::new().into_boxed_slice()),
        resolve_env_hash: [4u8; 16],
    };

    // Exhaustive destructure — fails to COMPILE if a `base: SemanticNodeId`
    // subject or a content/version field is added, or an axis removed.
    let MaterializationCacheKey {
        decl: _,
        projection_path: _,
        scope_axis: _,
        projection_mode: _,
        normalized_type_args: _,
        resolve_env_hash: _,
    } = &base;

    // Cross-owner reuse: an IDENTICAL canonical subject (any consumer
    // scope) is the SAME key — the key carries no scope dimension.
    assert_eq!(
        hash_of(&base),
        hash_of(&base.clone()),
        "an identical canonical subject must hash to the same slot (cross-owner reuse)",
    );

    // resolve_env_hash discriminates (R21 — not carried by the slot).
    let mut resolve_variant = base.clone();
    resolve_variant.resolve_env_hash = [99u8; 16];
    assert_ne!(
        hash_of(&base),
        hash_of(&resolve_variant),
        "resolve_env_hash must distinguish the MaterializationCacheKey slot",
    );

    // The slot's split env axes (project_identity / type_env / lib_env)
    // each discriminate.
    for variant_slot in [
        slot(9, [2u8; 16], [3u8; 16]),
        slot(1, [99u8; 16], [3u8; 16]),
        slot(1, [2u8; 16], [99u8; 16]),
    ] {
        let mut variant = base.clone();
        variant.decl = variant_slot;
        assert_ne!(
            hash_of(&base),
            hash_of(&variant),
            "each split env axis on the subject slot must distinguish the key",
        );
        // ProjectIdentity is a real dimension on the slot, not folded away.
        let _ = ProjectIdentity([1u8; 16]);
    }

    // The policy axis, mode, and projection path each discriminate.
    let mut axis_variant = base.clone();
    axis_variant.scope_axis = MaterializationScope::Nested;
    assert_ne!(
        hash_of(&base),
        hash_of(&axis_variant),
        "scope_axis must discriminate"
    );

    let mut mode_variant = base.clone();
    mode_variant.projection_mode = ProjectionMode::Expanded;
    assert_ne!(
        hash_of(&base),
        hash_of(&mode_variant),
        "projection_mode must discriminate"
    );

    let mut proj_variant = base.clone();
    proj_variant.projection_path = RouteDemand::pick(vec!["id".to_string()]);
    assert_ne!(
        hash_of(&base),
        hash_of(&proj_variant),
        "distinct projection paths must not alias onto one slot",
    );

    // A different canonical subject (different decl name) discriminates —
    // no over-share across semantically-distinct subjects.
    let mut subject_variant = base.clone();
    subject_variant.decl = ResolvedDeclSlotIdentity::type_slot(
        Arc::from("/w/ChatMessageProps.ts"),
        Arc::from("OtherProps"),
        1,
        [2u8; 16],
        [3u8; 16],
    );
    assert_ne!(
        hash_of(&base),
        hash_of(&subject_variant),
        "a different canonical subject must distinguish the key",
    );

    // `SymbolSpace` import touch — the slot's symbol_space is part of its
    // identity (type-space vs value-space subjects never collide).
    let _ = SymbolSpace::Type;
}

// ---------------------------------------------------------------------------
// Shape / materialize key-hygiene scanner
//
// `no_unsanctioned_semantic_node_id_in_shape_or_materialize_key` — scoped
// cache-key-hygiene over the shape/materialize derived-`Hash` cache keys
// (`ShapeCacheKey` + its `ShapeSubject` enum + `ShapeDemand`;
// `MaterializationCacheKey`). It forbids the content/version field
// types/markers and PINS the SemanticNodeId allow-list to EXACTLY TWO
// positions.
//
// MECHANISM — RECORDED SOURCE SCANNER, NOT a structural assert.
//
//   scanner_invariant=shape_materialize_key_no_unsanctioned_semantic_node_id
//   scanner_justification=Rust has no negative/forbidden-field-type trait
//     bound, so "forbid these field TYPES in a derived-Hash key while
//     allowing exactly two SemanticNodeId positions" has no compiler /
//     structural mechanism; the raw Hash16/HashValue aliases cannot
//     distinguish resolve_env_hash from whole_hash by type without a
//     key-safety newtype substrate, which is out of scope for this work.
//     A bounded source scan over the key struct/enum bodies is the
//     justified expression.
//   mechanism_ruling=binding architecture design ruling
//     (see `docs/arch/cache-key-guard-mechanism-rulings.md`): classify this
//     guard as a recorded scanner, NOT structural enforcement, because the raw
//     Hash16/HashValue aliases cannot distinguish resolve_env_hash from
//     whole_hash by type without an out-of-scope key-safety newtype substrate.
//   hardening_rounds=0
//   hardening_history=none — adopted at 0
//   structural_close_debt=KEY-SAFETY NEWTYPE SUBSTRATE (structural debt, P1).
//     This scanner — together with the g_misc2
//     `synthetic_carrier_explicit_deepen_routes_through_shape_cache_key`
//     scanner (the TWO `SemanticNodeId`-keyed scanners) — is a RECORDED
//     SOURCE SCANNER precisely because the structural mechanism does not
//     exist YET. The g_misc2 `no_carrier_verdict_db` scanner is NOT part of
//     this pair: it is a retired-symbol-absence scan and is NOT closed by
//     this substrate — newtyping hashes / sealing `SemanticNodeId` does not
//     prevent reintroducing a private `CarrierVerdictDb` / `carrier_verdicts`
//     symbol — so it REMAINS a recorded scanner (or would need its own
//     separate structural closure for retired-symbol absence). The TWO
//     `SemanticNodeId`-keyed scanners become STRUCTURAL (compiler-enforced,
//     not a text scan) and are DELETED once BOTH land:
//       (a) the env/content hash aliases `Hash16` / `HashValue` are NEWTYPED
//           so `resolve_env_hash` is type-distinct from `whole_hash` (a
//           content/version hash then cannot be written into a derived-`Hash`
//           query-identity key without a type error — no name-spelling scan
//           needed); AND
//       (b) `SemanticNodeId` and the `value_node` /
//           `SyntheticCarrierKey.value_node` tuple fields are SEALED (private
//           field + narrow constructors) so a raw `SemanticNodeId(u64)` /
//           `SemanticNodeId(<ident>.value_node)` construction is IMPOSSIBLE
//           outside the owning layer.
//     CLOSURE CRITERIA: when (a)+(b) land, the structural enforcement
//     replaces the TWO `SemanticNodeId`-keyed scanners and they are DELETED
//     — making them last-resort-WITH-A-PATH, not permanent
//     (`no_carrier_verdict_db` is unaffected and remains). The durable ruling notes
//     the structural close — a private `SemanticNodeId` tuple field — is the
//     ONLY design that dominates scanner-chasing (broadening a text scanner
//     into receiver/chained/binding data-flow spends effort without improving
//     the real guarantee, because structural confinement is the primary
//     cache-safety mechanism). Tracked: `docs/arch/key-safety-newtype-substrate-debt.md`.
//
// This scanner does NOT and CANNOT prove field-type identity through the
// type system; it is a bounded name-spelling scan over the key bodies. It
// is the BROADER scoped complement to the structural guards
// `member_value_node_subject_is_sealed_newtype_and_member_constructed` and
// `synthetic_binding_cache_subject_is_content_free_and_carrier_sealed`
// (which pin the individual `MemberValueNode` / `SyntheticBinding` arms by
// construction) — it is NOT a re-implementation of them. A BLANKET
// SemanticNodeId ban would be UNSOUND (it would catch legitimate
// intra-graph operands like `SemanticQueryKey::Instantiate.args`,
// `ProjectMember.base`, `ProjectPath.base`, `ResolveOverloadSet.callee`,
// or `IndexKey::TypeNode`), so the scope is the shape/materialize keys
// ONLY.

/// Content/version field markers forbidden in a shape/materialize
/// derived-`Hash` cache key: a present marker re-introduces a content /
/// version / identity hash (R6) or a bundled env hash (R21), or a
/// versioned/identity carrier that embeds one.
const SHAPE_MATERIALIZE_FORBIDDEN_MARKERS: &[&str] = &[
    "whole_hash",
    "content_hash",
    "parse_stable_hash",
    "fact_dep_signature",
    "project_config_hash",
    // Versioned / identity carriers that embed a content/version hash.
    // `ResolvedDeclSlotIdentity` does NOT contain the substring
    // `DeclIdentity` (no false-trip on the content-free slot type).
    "DeclIdentity",
    "VersionedDeclIdentity",
    // `HotTypeRef(SemanticNodeId)` is a graph-instance newtype — never a
    // content-free key field.
    "HotTypeRef",
    // A synthetic `value_node` field would key on the store-relative arena
    // ordinal — the R6 violation the content-free `SyntheticBindingId`
    // removed.
    "value_node",
];

/// Replace `//` line comments and `/* ... */` block comments (with
/// nesting) with equivalent whitespace, preserving newlines so line
/// numbers stay stable, and skipping comment-like sequences inside
/// regular and raw string literals. This is the SAME comment-stripping
/// discipline the sibling retired-symbol scanner
/// (`no_carrier_verdict_db.rs`) applies — the key-hygiene scan must strip
/// BOTH comment kinds before matching, so a `/* block comment */`
/// mentioning a forbidden marker (or `MemberShapeNodeSubject`, or
/// `SemanticNodeId`) cannot false-satisfy a check.
fn strip_comments(src: &str) -> String {
    let bytes = src.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let n = bytes.len();
    let mut i = 0usize;
    while i < n {
        let c = bytes[i];
        // Raw string: r"..."  /  r#"..."#  /  r##"..."##  ...
        if c == b'r' {
            let mut j = i + 1;
            let mut hashes = 0usize;
            while j < n && bytes[j] == b'#' {
                hashes += 1;
                j += 1;
            }
            if j < n && bytes[j] == b'"' {
                out.extend_from_slice(&bytes[i..=j]);
                let close: Vec<u8> = std::iter::once(b'"')
                    .chain(std::iter::repeat_n(b'#', hashes))
                    .collect();
                let mut k = j + 1;
                while k + close.len() <= n {
                    if &bytes[k..k + close.len()] == close.as_slice() {
                        out.extend_from_slice(&bytes[(j + 1)..(k + close.len())]);
                        i = k + close.len();
                        break;
                    }
                    out.push(bytes[k]);
                    k += 1;
                }
                if k + close.len() > n {
                    out.extend_from_slice(&bytes[(j + 1)..n]);
                    i = n;
                }
                continue;
            }
        }
        // Regular string literal "..." with \" escape handling.
        if c == b'"' {
            out.push(b'"');
            let mut k = i + 1;
            while k < n {
                if bytes[k] == b'\\' && k + 1 < n {
                    out.push(bytes[k]);
                    out.push(bytes[k + 1]);
                    k += 2;
                    continue;
                }
                if bytes[k] == b'"' {
                    out.push(b'"');
                    k += 1;
                    break;
                }
                out.push(bytes[k]);
                k += 1;
            }
            i = k;
            continue;
        }
        // Line comment //
        if c == b'/' && i + 1 < n && bytes[i + 1] == b'/' {
            let mut k = i;
            while k < n && bytes[k] != b'\n' {
                out.push(b' ');
                k += 1;
            }
            i = k;
            continue;
        }
        // Block comment /* ... */ with nesting support.
        if c == b'/' && i + 1 < n && bytes[i + 1] == b'*' {
            let mut depth = 1u32;
            out.push(b' ');
            out.push(b' ');
            let mut k = i + 2;
            while k < n && depth > 0 {
                if k + 1 < n && bytes[k] == b'/' && bytes[k + 1] == b'*' {
                    depth += 1;
                    out.push(b' ');
                    out.push(b' ');
                    k += 2;
                    continue;
                }
                if k + 1 < n && bytes[k] == b'*' && bytes[k + 1] == b'/' {
                    depth -= 1;
                    out.push(b' ');
                    out.push(b' ');
                    k += 2;
                    continue;
                }
                if bytes[k] == b'\n' {
                    out.push(b'\n');
                } else {
                    out.push(b' ');
                }
                k += 1;
            }
            i = k;
            continue;
        }
        out.push(c);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Extract the body of an enum variant `VariantName { ... }` from
/// `source` — the inner text between the variant's `{` and the matching
/// `}` (braces excluded). The caller MUST pass a comment-free `source`
/// (run it through [`strip_comments`] first), so a doc/inline/block
/// comment mentioning a forbidden marker can never false-trip the scan.
/// Returns `None` if the variant or its braces are missing/unbalanced.
fn extract_enum_variant_block(source: &str, variant: &str) -> Option<String> {
    let needle = format!("{variant} {{");
    let start = source.find(&needle)?;
    let bytes = source.as_bytes();
    // Body opens at the `{` that is part of `needle`.
    let body_open = start + needle.len() - 1;
    let mut depth = 0usize;
    for (offset, &b) in bytes[body_open..].iter().enumerate() {
        match b {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    let begin = body_open + 1;
                    let end = body_open + offset;
                    return Some(source[begin..end].to_string());
                }
            }
            _ => {}
        }
    }
    None
}

/// True iff `tok` is the identifier-boundary token `ident` somewhere in
/// `body` (so `SemanticNodeId` does not match inside `SemanticNodeIdFoo`).
fn body_has_ident(body: &str, ident: &str) -> bool {
    body.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .any(|t| t == ident)
}

/// Split a comment-free struct/enum-arm body into its `(field_name,
/// field_type)` declarations. Fields are separated by top-level commas;
/// the split is depth-aware over `< > ( ) [ ] { }` so a comma inside a
/// generic argument list (`BoundedMap<K, V>`) or a tuple does NOT split a
/// field. Within a field, the NAME is the leading identifier before the
/// first top-level `:` (with any `pub` / `pub(crate)` / `pub(...)`
/// visibility prefix stripped), and the TYPE is the remainder. A field
/// with no `:` (a tuple-struct positional, or a bare marker) yields an
/// empty name and the whole field text as the type. Returns the list in
/// source order.
///
/// This is the substrate for the EXACT-field allow-list: the scanner
/// pins the sanctioned `SemanticNodeId` positions by exact field NAME and
/// TYPE, not by "appears somewhere in the body", so a renamed args field,
/// a duplicate sealed-newtype field, or the newtype reused in a different
/// body cannot satisfy the sanctioned slot.
fn split_struct_fields(body: &str) -> Vec<(String, String)> {
    let mut fields: Vec<(String, String)> = Vec::new();
    let mut depth = 0i32;
    let mut current = String::new();
    for ch in body.chars() {
        match ch {
            '<' | '(' | '[' | '{' => {
                depth += 1;
                current.push(ch);
            }
            '>' | ')' | ']' | '}' => {
                depth -= 1;
                current.push(ch);
            }
            ',' if depth == 0 => {
                push_field(&mut fields, &current);
                current.clear();
            }
            _ => current.push(ch),
        }
    }
    push_field(&mut fields, &current);
    fields
}

/// Parse one field segment into `(name, type)` and append it if
/// non-blank. Splits at the first top-level `:`; strips a leading `pub`
/// / `pub(crate)` / `pub(...)` visibility prefix from the name side.
fn push_field(out: &mut Vec<(String, String)>, segment: &str) {
    let trimmed = segment.trim();
    if trimmed.is_empty() {
        return;
    }
    // Find the first top-level `:` (depth-aware so `Arc<[T; N]>` etc. do
    // not interfere — those carry no `:` anyway, but stay defensive).
    let mut depth = 0i32;
    let mut colon: Option<usize> = None;
    for (idx, ch) in trimmed.char_indices() {
        match ch {
            '<' | '(' | '[' | '{' => depth += 1,
            '>' | ')' | ']' | '}' => depth -= 1,
            ':' if depth == 0 => {
                // A `::` path separator is not a field-name terminator.
                let next_is_colon = trimmed[idx + 1..].starts_with(':');
                let prev_is_colon = trimmed[..idx].ends_with(':');
                if !next_is_colon && !prev_is_colon {
                    colon = Some(idx);
                    break;
                }
            }
            _ => {}
        }
    }
    match colon {
        Some(c) => {
            let raw_name = trimmed[..c].trim();
            // Strip a visibility prefix: `pub`, `pub(crate)`, `pub(...)`.
            let name = strip_visibility(raw_name).trim().to_string();
            let ty = normalize_ws(trimmed[c + 1..].trim());
            out.push((name, ty));
        }
        None => {
            // Tuple-positional / bare marker — no name.
            out.push((String::new(), normalize_ws(trimmed)));
        }
    }
}

/// Strip a leading `pub` / `pub(crate)` / `pub(in path)` visibility
/// modifier from a field-name fragment.
///
/// `pub` is stripped ONLY when it is the visibility TOKEN — the whole
/// fragment (`pub`), or `pub` immediately followed by whitespace
/// (`pub field`) or `(` (`pub(crate) field`). It is NEVER stripped as a
/// bare prefix of a longer identifier: a field named `pubnode` /
/// `pubnormalized_type_args` is a DISTINCT identifier, not a `pub`-qualified
/// `node` / `normalized_type_args`. A bare `strip_prefix("pub")` would
/// mis-credit `pubnode` as the sanctioned `node` field — a false negative
/// the visibility-token boundary closes.
fn strip_visibility(name: &str) -> &str {
    let n = name.trim_start();
    let Some(rest) = n.strip_prefix("pub") else {
        return n;
    };
    // `pub` must be the visibility TOKEN, not the prefix of a longer
    // identifier (`pubnode`). Accept only when the char after `pub` cannot
    // extend the `pub` identifier: end-of-fragment, whitespace, or `(`.
    match rest.chars().next() {
        None => "", // the whole fragment was exactly `pub`
        Some('(') => {
            // `pub(crate)` / `pub(in path)` — skip to the matching `)`.
            if let Some(after_paren) = rest.strip_prefix('(') {
                if let Some(close) = after_paren.find(')') {
                    return after_paren[close + 1..].trim_start();
                }
            }
            rest.trim_start()
        }
        Some(c) if c.is_whitespace() => rest.trim_start(),
        // `pub` is a bare prefix of a longer identifier (`pubnode`) — NOT a
        // visibility token. Return the original identifier untouched.
        Some(_) => n,
    }
}

/// Collapse all internal whitespace runs to single spaces and trim — so a
/// multi-line field type compares by its canonical spelling.
fn normalize_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// True iff `ty` is exactly the `Arc<[SemanticNodeId]>` instantiation-args
/// type (whitespace-tolerant). This is the SANCTIONED args-position TYPE
/// (`MaterializationCacheKey.normalized_type_args`).
fn type_is_args_list(ty: &str) -> bool {
    ty.split_whitespace().collect::<String>() == "Arc<[SemanticNodeId]>"
}

/// Total `SemanticNodeId` identifier-boundary token occurrences in a key
/// body.
fn count_semantic_node_id_tokens(body: &str) -> usize {
    count_ident_tokens(body, "SemanticNodeId")
}

/// Total identifier-boundary occurrences of `ident` in `body` (so
/// `SemanticNodeId` does not count inside `SemanticNodeIdFoo`, and
/// `MemberShapeNodeSubject` does not count inside
/// `MemberShapeNodeSubjectExt`).
fn count_ident_tokens(body: &str, ident: &str) -> usize {
    body.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .filter(|t| *t == ident)
        .count()
}

/// A single field expectation in an EXACT body inventory: the required
/// field NAME, and — when the field is type-critical — its EXACT
/// normalized type spelling (`Some(ty)`). A `None` type pins only the
/// presence of the named field, not its type.
struct ExpectedField {
    name: &'static str,
    /// `Some(ty)` ⇒ the field's normalized type MUST equal `ty` exactly
    /// (used to pin the SemanticNodeId-bearing fields:
    /// `normalized_type_args: Arc<[SemanticNodeId]>`,
    /// `node: MemberShapeNodeSubject`). `None` ⇒ name-only.
    ty: Option<&'static str>,
}

/// Assert a scanned body's FIELD-NAME inventory equals the EXACT
/// expected set (no missing, no extra, order-independent), and that each
/// type-critical field carries its EXACT expected normalized type. A body
/// that grows an unexpected field — including an unexpected
/// `MemberShapeNodeSubject`-typed field in a NON-member body (the exact
/// global newtype pin) — FAILS here rather than leaning on the violation
/// pass. Returns the human-readable failures; empty == the inventory
/// matches exactly.
///
/// The caller MUST pass a comment-free `body` (the guard strips comments
/// up front).
fn exact_field_inventory_failures(
    key_name: &str,
    body: &str,
    expected: &[ExpectedField],
) -> Vec<String> {
    let mut failures = Vec::new();
    let fields = split_struct_fields(body);
    let actual_names: Vec<&str> = fields.iter().map(|(n, _)| n.as_str()).collect();

    // Missing expected fields.
    for ef in expected {
        if !actual_names.contains(&ef.name) {
            failures.push(format!(
                "`{key_name}`: MISSING expected field `{}` (exact inventory)",
                ef.name
            ));
        }
    }
    // Unexpected extra fields (anything not in the expected name set).
    for name in &actual_names {
        if !expected.iter().any(|ef| ef.name == *name) {
            failures.push(format!(
                "`{key_name}`: UNEXPECTED field `{name}` not in the exact inventory \
                 {:?} — a new/renamed field changes the key's content/identity surface \
                 and must be reviewed",
                expected.iter().map(|e| e.name).collect::<Vec<_>>(),
            ));
        }
    }
    // Type-critical fields: the named field's normalized type must match
    // EXACTLY (and appear exactly once).
    for ef in expected {
        let Some(want_ty) = ef.ty else { continue };
        let matching: Vec<&(String, String)> =
            fields.iter().filter(|(n, _)| n == ef.name).collect();
        match matching.as_slice() {
            [(_, ty)] => {
                if normalize_ws(ty) != want_ty {
                    failures.push(format!(
                        "`{key_name}`: field `{}` must be typed EXACTLY `{want_ty}` \
                         (type-critical); found `{}`",
                        ef.name,
                        normalize_ws(ty),
                    ));
                }
            }
            [] => { /* missing already reported above */ }
            many => failures.push(format!(
                "`{key_name}`: field `{}` declared {} times — a type-critical field \
                 must appear exactly once",
                ef.name,
                many.len(),
            )),
        }
    }
    failures
}

/// Enumerate the top-level variant NAMES declared in a comment-free
/// `enum` body — the leading identifier of each top-level
/// (depth-0-comma-separated) segment. Handles `Name { .. }`, `Name(..)`,
/// and unit `Name` variants. Used by the CLOSED variant-inventory
/// assertion: a NEW `ShapeSubject` arm must FAIL the guard until it is
/// consciously added to the sanctioned set, so a future
/// `ShapeSubject::Other { node: SemanticNodeId }` cannot slip past a
/// scanner that only knew the original three arms.
fn enum_variant_names(enum_body: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    let mut depth = 0i32;
    let mut current = String::new();
    for ch in enum_body.chars() {
        match ch {
            '<' | '(' | '[' | '{' => {
                depth += 1;
                current.push(ch);
            }
            '>' | ')' | ']' | '}' => {
                depth -= 1;
                current.push(ch);
            }
            ',' if depth == 0 => {
                push_variant_name(&mut names, &current);
                current.clear();
            }
            _ => current.push(ch),
        }
    }
    push_variant_name(&mut names, &current);
    names
}

/// Append the leading identifier of an enum-variant segment (the variant
/// NAME), if the segment is non-blank and starts with an identifier.
///
/// Any leading outer attribute(s) (`#[cfg(...)]`, possibly multiple,
/// possibly multi-line) are stripped FIRST, so an ATTRIBUTED variant
/// (`#[cfg(test)] Other { .. }`) still surfaces its name. Without the
/// attribute strip an attributed, content-free arm would yield no name
/// and stay invisible to the closed-inventory assert — an "any new
/// variant fails inventory" escape the attribute strip closes.
fn push_variant_name(out: &mut Vec<String>, segment: &str) {
    let stripped = strip_leading_outer_attributes(segment);
    let trimmed = stripped.trim_start();
    let name: String = trimmed
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    if !name.is_empty() {
        out.push(name);
    }
}

/// Strip every leading outer attribute (`#[ ... ]`, possibly multiple,
/// possibly spanning newlines) from a segment, returning the remainder
/// (the variant declaration). Whitespace between attributes is skipped.
/// A `#` that does NOT open a `[ ... ]` attribute (or an unbalanced one)
/// stops the strip — the remainder is returned from that point so a
/// malformed segment never silently swallows the whole text.
fn strip_leading_outer_attributes(segment: &str) -> &str {
    let mut rest = segment.trim_start();
    while let Some(after_hash) = rest.strip_prefix('#') {
        let after_hash = after_hash.trim_start();
        let Some(inner) = after_hash.strip_prefix('[') else {
            // `#` not followed by `[` — not an outer attribute; stop.
            break;
        };
        // Find the matching `]` (depth-aware so a nested `[ ]` inside the
        // attribute token tree does not end it early).
        let bytes = inner.as_bytes();
        let mut depth = 1i32;
        let mut close: Option<usize> = None;
        for (idx, &b) in bytes.iter().enumerate() {
            match b {
                b'[' => depth += 1,
                b']' => {
                    depth -= 1;
                    if depth == 0 {
                        close = Some(idx);
                        break;
                    }
                }
                _ => {}
            }
        }
        match close {
            Some(idx) => rest = inner[idx + 1..].trim_start(),
            // Unbalanced attribute — stop, return what is left.
            None => break,
        }
    }
    rest
}

/// Compute the shape/materialize key-hygiene violations for one key body.
///
/// - any forbidden content/version marker present ⇒ violation (R6/R21);
/// - an `Arc<[SemanticNodeId]>` args field whose NAME is not the single
///   `sanctioned_args_field` ⇒ violation (the args allow-list is pinned
///   by exact field NAME, so a renamed `rogue_nodes:
///   Arc<[SemanticNodeId]>`, or a SECOND args list, is rejected even if
///   one materialize args slot is nominally permitted);
/// - any BARE `SemanticNodeId` (a `SemanticNodeId` token NOT accounted for
///   by the sanctioned-named args list) ⇒ violation: the only sanctioned
///   subject-position representation is the sealed `MemberShapeNodeSubject`
///   newtype (which spells `MemberShapeNodeSubject` in the body, not
///   `SemanticNodeId`), so a literal `SemanticNodeId` subject/field is
///   unsanctioned.
///
/// `sanctioned_args_field` is `Some(field_name)` when this body may carry
/// EXACTLY ONE `Arc<[SemanticNodeId]>` args list, and only under that
/// field name (`Some("normalized_type_args")` for the materialize key);
/// `None` for the shape keys (which carry no args list at all). Returns
/// the human-readable violations; empty == compliant.
fn shape_materialize_key_violations(
    key_name: &str,
    body: &str,
    sanctioned_args_field: Option<&str>,
) -> Vec<String> {
    let body = strip_comments(body);
    let mut violations = Vec::new();
    for marker in SHAPE_MATERIALIZE_FORBIDDEN_MARKERS {
        if body_has_ident(&body, marker) {
            violations.push(format!(
                "`{key_name}`: FORBIDDEN content/version marker `{marker}` present \
                 (R6/R21 — a shape/materialize key must be content-free)"
            ));
        }
    }

    // Pin the args allow-list to an EXACT FIELD NAME. Every
    // `Arc<[SemanticNodeId]>`-typed field must be the single sanctioned
    // field; any other args-typed field (a renamed `rogue_nodes`, a
    // duplicate, or a second list) is unsanctioned.
    let fields = split_struct_fields(&body);
    let mut sanctioned_args_count = 0usize;
    for (name, ty) in &fields {
        if type_is_args_list(ty) {
            match sanctioned_args_field {
                Some(allowed) if name == allowed => sanctioned_args_count += 1,
                _ => violations.push(format!(
                    "`{key_name}`: UNSANCTIONED `Arc<[SemanticNodeId]>` args field \
                     `{name}` — the SemanticNodeId args allow-list is pinned to the \
                     EXACT field name {sanctioned:?}; a renamed/duplicate/extra args \
                     list is rejected (R6/R21)",
                    sanctioned = sanctioned_args_field,
                )),
            }
        }
    }
    if sanctioned_args_count > 1 {
        violations.push(format!(
            "`{key_name}`: {sanctioned_args_count} fields named \
             {sanctioned_args_field:?} carry `Arc<[SemanticNodeId]>` — the sanctioned \
             args position is EXACT (one field), not a name that may repeat",
        ));
    }

    // Every `Arc<[SemanticNodeId]>` contributes exactly one SemanticNodeId
    // token. Any token beyond the sanctioned-named args list is a BARE
    // SemanticNodeId subject/field — unsanctioned.
    let total = count_semantic_node_id_tokens(&body);
    let bare = total.saturating_sub(sanctioned_args_count.min(1));
    if bare > 0 {
        violations.push(format!(
            "`{key_name}`: {bare} unsanctioned BARE `SemanticNodeId` position(s) in the \
             key body — a shape/materialize key may carry a `SemanticNodeId` ONLY as \
             `MaterializationCacheKey.normalized_type_args` (`Arc<[SemanticNodeId]>`) or \
             via the sealed `MemberShapeNodeSubject` newtype (which keeps the ordinal \
             module-private and does NOT spell `SemanticNodeId` in the body). A literal \
             `SemanticNodeId` subject/field re-opens the graph-instance ordinal key the \
             content-free identities removed (R6)"
        ));
    }
    violations
}

/// Self-test: the shape/materialize key-hygiene predicate must provably
/// FLAG every forbidden marker, FLAG a bare `SemanticNodeId` subject, FLAG
/// an over-the-allow-list args position, FLAG a RENAMED / DUPLICATE args
/// field even when one args slot is permitted, strip BLOCK comments as
/// well as line comments, and ACCEPT the two sanctioned positions.
/// Without this discriminator the scanner below is a stub.
#[test]
fn shape_materialize_key_hygiene_predicate_discriminates() {
    // The materialize key permits EXACTLY ONE args list, under the exact
    // field name `normalized_type_args`; the shape keys permit none.
    const MAT_ARGS: Option<&str> = Some("normalized_type_args");
    const NO_ARGS: Option<&str> = None;

    // (1) Each forbidden marker is independently flagged.
    for marker in SHAPE_MATERIALIZE_FORBIDDEN_MARKERS {
        let body = format!("pub scope: Arc<str>, pub bad: {marker},");
        let v = shape_materialize_key_violations("FixtureKey", &body, NO_ARGS);
        assert!(
            v.iter().any(|m| m.contains(marker)),
            "predicate must flag forbidden marker `{marker}`; got {v:?}",
        );
    }

    // (2) A bare `SemanticNodeId` subject/field is flagged.
    let bare_subject = "pub scope: Arc<str>, pub node: SemanticNodeId,";
    let v = shape_materialize_key_violations("FixtureKey", bare_subject, NO_ARGS);
    assert!(
        v.iter().any(|m| m.contains("BARE `SemanticNodeId`")),
        "predicate must flag a bare SemanticNodeId subject/field; got {v:?}",
    );

    // (3) The sanctioned args position is ACCEPTED when allowed under its
    //     exact name, and REJECTED when the body carries no args slot.
    let args_body = "pub decl: ResolvedDeclSlotIdentity, \
                     pub normalized_type_args: Arc<[SemanticNodeId]>, \
                     pub resolve_env_hash: HashValue,";
    assert!(
        shape_materialize_key_violations("MaterializationCacheKey", args_body, MAT_ARGS).is_empty(),
        "predicate must ACCEPT the single sanctioned `normalized_type_args: \
         Arc<[SemanticNodeId]>` args list when one is allowed",
    );
    let v = shape_materialize_key_violations("ShapeKeyShaped", args_body, NO_ARGS);
    assert!(
        !v.is_empty(),
        "predicate must REJECT an args list on a body whose allow-list is empty",
    );

    // (4) The sanctioned member-subject representation (the sealed newtype,
    //     which spells `MemberShapeNodeSubject`, NOT `SemanticNodeId`) is
    //     ACCEPTED — no false-trip on the legitimate seal.
    let sealed_member = "pub scope: Arc<str>, pub node: MemberShapeNodeSubject,";
    assert!(
        shape_materialize_key_violations("ShapeSubject", sealed_member, NO_ARGS).is_empty(),
        "predicate must ACCEPT the sealed `MemberShapeNodeSubject` representation \
         (it does not spell `SemanticNodeId` in the body)",
    );

    // (5) A SECOND args position is flagged (exact, not open).
    let two_args = "pub a: Arc<[SemanticNodeId]>, pub b: Arc<[SemanticNodeId]>,";
    let v = shape_materialize_key_violations("MaterializationCacheKey", two_args, MAT_ARGS);
    assert!(
        v.iter()
            .any(|m| m.contains("UNSANCTIONED `Arc<[SemanticNodeId]>` args field")),
        "predicate must flag an args field NOT named `normalized_type_args` (`a`/`b`); \
         got {v:?}",
    );

    // (6) A clean content-free body with neither marker nor SemanticNodeId
    //     is ACCEPTED.
    let clean = "pub decl: ResolvedDeclSlotIdentity, pub resolve_env_hash: HashValue, \
                 pub scope_axis: MaterializationScope,";
    assert!(
        shape_materialize_key_violations("CleanKey", clean, MAT_ARGS).is_empty(),
        "predicate must accept a clean content-free body",
    );

    // (7) The scoping control: a SemanticNodeId operand body that is NOT a
    //     shape/materialize key — the predicate only runs against the
    //     shape/materialize bodies, so an `Instantiate`-style operand
    //     (`base: SemanticNodeId, args: Arc<[SemanticNodeId]>`) is NEVER
    //     handed to it. Confirm here that scoping by NOT calling the
    //     predicate on such a body is the design (the guard test below
    //     scans ONLY the four shape/materialize bodies).
    let instantiate_operand = "pub base: SemanticNodeId, pub args: Arc<[SemanticNodeId]>,";
    // If it WERE (wrongly) treated as a no-args-allowed body it would flag —
    // proving a blanket ban over this operand would be a false positive,
    // which is exactly why the guard scopes to the shape/materialize keys.
    assert!(
        !shape_materialize_key_violations(
            "InstantiateOperandAsIfShapeKey",
            instantiate_operand,
            NO_ARGS
        )
        .is_empty(),
        "control: an `Instantiate`-style operand WOULD trip a no-args-allowed scan — \
         which is why the guard must NOT scan intra-graph operands; it scopes to the \
         shape/materialize keys only",
    );

    // (8) a RENAMED args field (`rogue_nodes: Arc<[SemanticNodeId]>`) is
    //     REJECTED even when ONE materialize args slot is permitted — the
    //     allow-list is pinned to the EXACT field name, so a future rogue
    //     args field cannot consume the one sanctioned slot.
    let renamed_args = "pub decl: ResolvedDeclSlotIdentity, \
                        pub rogue_nodes: Arc<[SemanticNodeId]>, \
                        pub resolve_env_hash: HashValue,";
    let v = shape_materialize_key_violations("MaterializationCacheKey", renamed_args, MAT_ARGS);
    assert!(
        v.iter().any(
            |m| m.contains("UNSANCTIONED `Arc<[SemanticNodeId]>` args field")
                && m.contains("rogue_nodes")
        ),
        "predicate must REJECT a renamed args field `rogue_nodes` even when one \
         `normalized_type_args` slot is allowed; got {v:?}",
    );

    // (9) a DUPLICATE sanctioned-name args field is REJECTED —
    //     two `normalized_type_args` fields must not both satisfy the one
    //     sanctioned position.
    let duplicate_args = "pub decl: ResolvedDeclSlotIdentity, \
                          pub normalized_type_args: Arc<[SemanticNodeId]>, \
                          pub normalized_type_args: Arc<[SemanticNodeId]>, \
                          pub resolve_env_hash: HashValue,";
    let v = shape_materialize_key_violations("MaterializationCacheKey", duplicate_args, MAT_ARGS);
    assert!(
        !v.is_empty(),
        "predicate must REJECT a DUPLICATE `normalized_type_args` field — the \
         sanctioned position is exactly one field, and the extra SemanticNodeId token \
         surfaces as a bare/duplicate violation; got {v:?}",
    );

    // (10) a BLOCK comment mentioning a forbidden marker /
    //      `SemanticNodeId` must NOT satisfy or trip a check (block
    //      comments are stripped, not just `//` line comments).
    let block_comment_decoy = "pub decl: ResolvedDeclSlotIdentity, \
                               /* whole_hash SemanticNodeId MemberShapeNodeSubject */ \
                               pub resolve_env_hash: HashValue,";
    assert!(
        shape_materialize_key_violations("CleanKeyWithBlockComment", block_comment_decoy, MAT_ARGS)
            .is_empty(),
        "predicate must strip BLOCK comments: a `/* whole_hash ... */` decoy must not \
         trip the forbidden-marker scan",
    );
}

/// Scoped cache-key-hygiene guard (RECORDED SOURCE SCANNER — see the
/// module-level record above; this is NOT structural enforcement). The
/// shape/materialize derived-`Hash` cache keys — `ShapeCacheKey`, its
/// `ShapeSubject` enum, `ShapeDemand`, and `MaterializationCacheKey` —
/// must carry NONE of the forbidden content/version markers
/// (`whole_hash` / `content_hash` / `parse_stable_hash` /
/// `fact_dep_signature` / `project_config_hash` / `DeclIdentity` /
/// `VersionedDeclIdentity` / `HotTypeRef` / `value_node`) and may carry a
/// `SemanticNodeId` in EXACTLY TWO sanctioned positions:
///   1. `MaterializationCacheKey.normalized_type_args` (an
///      `Arc<[SemanticNodeId]>` instantiation-args list), and
///   2. the sealed `MemberShapeNodeSubject` newtype keying
///      `ShapeSubject::MemberValueNode.node` (the newtype keeps the arena
///      ordinal module-private and spells `MemberShapeNodeSubject` in the
///      body, NOT `SemanticNodeId`).
///
/// The allow-list is EXACT: a THIRD `SemanticNodeId` position (a bare
/// subject/field, or a second args list) is REJECTED until explicitly
/// added here.
///
/// SCOPE: shape/materialize keys ONLY. A blanket `SemanticNodeId` ban
/// would be unsound — it would flag the legitimate intra-graph operands
/// (`SemanticQueryKey::Instantiate.args` / `ProjectMember.base` /
/// `ProjectPath.base` / `ResolveOverloadSet.callee` / `IndexKey::TypeNode`),
/// which are NOT shape/materialize keys and stay allowed.
/// The sanctioned `ShapeSubject` variant inventory. The scanner extracts
/// the `MemberValueNode` / `SyntheticBinding` arms by name, so a NEW arm
/// would be INVISIBLE to that extraction. The closed inventory assertion
/// FAILS the guard the moment the enum grows a 3rd arm — forcing a
/// conscious review (and an explicit add here) of whether the new arm
/// carries an unsanctioned `SemanticNodeId`.
const SANCTIONED_SHAPE_SUBJECT_VARIANTS: &[&str] = &["MemberValueNode", "SyntheticBinding"];

#[test]
fn no_unsanctioned_semantic_node_id_in_shape_or_materialize_key() {
    // Strip ALL comments (line AND block) from the production sources
    // ONCE, up front, so every downstream brace/variant extraction
    // operates on comment-free text — a `/* ... */` (or `//`) comment
    // mentioning a forbidden marker, `SemanticNodeId`, or
    // `MemberShapeNodeSubject` can never satisfy or trip a check.
    let caches = strip_comments(&read_file(
        "crates/verter_session/src/component_meta_caches.rs",
    ));
    let materialize = strip_comments(&read_file(
        "crates/verter_session/src/component_meta_materialize.rs",
    ));

    // The production sources must be non-empty AND must actually contain
    // the struct/enum definitions we are about to scan. A partial /
    // truncated read that dropped these definitions would otherwise let
    // the guard pass vacuously (every `extract_*` returns `None` → the
    // `.expect()`s below would fire, but assert the sentinels explicitly so
    // the failure mode is a precise "source did not contain X", not a
    // brace-walk panic).
    assert!(
        !caches.trim().is_empty() && !materialize.trim().is_empty(),
        "key-hygiene GUARD: a production source read empty — the scan would be vacuous",
    );
    for (needle, src, file) in [
        (
            "pub struct ShapeCacheKey {",
            &caches,
            "component_meta_caches.rs",
        ),
        (
            "pub struct ShapeDemand {",
            &caches,
            "component_meta_caches.rs",
        ),
        (
            "pub enum ShapeSubject {",
            &caches,
            "component_meta_caches.rs",
        ),
        (
            "pub struct MaterializationCacheKey {",
            &materialize,
            "component_meta_materialize.rs",
        ),
    ] {
        assert!(
            src.contains(needle),
            "key-hygiene GUARD: sentinel `{needle}` is ABSENT from {file} — the scanner \
             would not actually scan it (a guard that scans a source must prove the \
             expected definition was present)",
        );
    }

    // The four shape/materialize derived-`Hash` cache-key bodies.
    let shape_cache_key = extract_brace_block(&caches, "pub struct ShapeCacheKey {").expect(
        "key-hygiene GUARD: could not locate `pub struct ShapeCacheKey` body in \
         component_meta_caches.rs",
    );
    let shape_demand = extract_brace_block(&caches, "pub struct ShapeDemand {").expect(
        "key-hygiene GUARD: could not locate `pub struct ShapeDemand` body in \
         component_meta_caches.rs",
    );
    let materialization_key =
        extract_brace_block(&materialize, "pub struct MaterializationCacheKey {").expect(
            "key-hygiene GUARD: could not locate `pub struct MaterializationCacheKey` body in \
             component_meta_materialize.rs",
        );
    // Scope variant-arm extraction to the `ShapeSubject` enum body first, so
    // a variant needle cannot match an unrelated earlier site.
    let shape_subject_enum = extract_brace_block(&caches, "pub enum ShapeSubject {").expect(
        "key-hygiene GUARD: could not locate `pub enum ShapeSubject` body in \
         component_meta_caches.rs",
    );

    // The `ShapeSubject` variant set is CLOSED. Assert the enum carries
    // EXACTLY the sanctioned arms — a new arm (e.g.
    // `Other { node: SemanticNodeId }`) FAILS here until it is consciously
    // reviewed and added to `SANCTIONED_SHAPE_SUBJECT_VARIANTS`, so it can
    // never slip past the per-arm extraction (which only knows the named
    // arms).
    let actual_variants = enum_variant_names(&shape_subject_enum);
    assert_eq!(
        actual_variants
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        SANCTIONED_SHAPE_SUBJECT_VARIANTS,
        "key-hygiene GUARD (closed inventory): the `ShapeSubject` enum must carry \
         EXACTLY the sanctioned variants {SANCTIONED_SHAPE_SUBJECT_VARIANTS:?} in order. \
         A new/renamed/removed arm changes the SemanticNodeId attack surface and must be \
         reviewed (and, if sanctioned, added to `SANCTIONED_SHAPE_SUBJECT_VARIANTS`) — \
         the per-arm extraction below only knows the named arms, so an unreviewed arm \
         would be invisible to it. Found: {actual_variants:?}",
    );

    let member_value_arm = extract_enum_variant_block(&shape_subject_enum, "MemberValueNode")
        .expect(
            "key-hygiene GUARD: could not locate the `ShapeSubject::MemberValueNode` variant body \
             in the ShapeSubject enum",
        );
    let synthetic_binding_arm = extract_enum_variant_block(&shape_subject_enum, "SyntheticBinding")
        .expect(
            "key-hygiene GUARD: could not locate the `ShapeSubject::SyntheticBinding` variant body \
             in the ShapeSubject enum",
        );

    // Every extracted body's FIELD-NAME inventory must equal the EXACT
    // expected set (no missing, no extra), and the type-critical fields
    // must carry their EXACT normalized type. An exact inventory is
    // stronger than mere presence checks — a body that GROWS an unexpected
    // field (e.g. a stray `MemberShapeNodeSubject`-typed field, the global
    // newtype pin) FAILS here rather than relying on the violation pass —
    // and it doubles as the anti-vacuity sentinel (a silently-empty
    // extracted body has a mismatched inventory and trips).
    let mut inventory_failures = Vec::new();
    inventory_failures.extend(exact_field_inventory_failures(
        "ShapeCacheKey",
        &shape_cache_key,
        &[
            ExpectedField {
                name: "subject",
                ty: None,
            },
            ExpectedField {
                name: "demand",
                ty: None,
            },
        ],
    ));
    inventory_failures.extend(exact_field_inventory_failures(
        "ShapeDemand",
        &shape_demand,
        &[
            ExpectedField {
                name: "path",
                ty: None,
            },
            ExpectedField {
                name: "terminal_context",
                ty: None,
            },
            ExpectedField {
                name: "key_filter",
                ty: None,
            },
            ExpectedField {
                name: "surface",
                ty: None,
            },
        ],
    ));
    inventory_failures.extend(exact_field_inventory_failures(
        "MaterializationCacheKey",
        &materialization_key,
        &[
            ExpectedField {
                name: "decl",
                ty: None,
            },
            ExpectedField {
                name: "projection_path",
                ty: None,
            },
            ExpectedField {
                name: "scope_axis",
                ty: None,
            },
            ExpectedField {
                name: "projection_mode",
                ty: None,
            },
            // Type-critical: the ONLY sanctioned args-list position.
            ExpectedField {
                name: "normalized_type_args",
                ty: Some("Arc<[SemanticNodeId]>"),
            },
            ExpectedField {
                name: "resolve_env_hash",
                ty: None,
            },
        ],
    ));
    inventory_failures.extend(exact_field_inventory_failures(
        "ShapeSubject::MemberValueNode",
        &member_value_arm,
        &[
            ExpectedField {
                name: "scope",
                ty: None,
            },
            // Type-critical: the SECOND sanctioned SemanticNodeId position,
            // sealed behind the `MemberShapeNodeSubject` newtype.
            ExpectedField {
                name: "node",
                ty: Some("MemberShapeNodeSubject"),
            },
        ],
    ));
    inventory_failures.extend(exact_field_inventory_failures(
        "ShapeSubject::SyntheticBinding",
        &synthetic_binding_arm,
        &[
            ExpectedField {
                name: "id",
                ty: None,
            },
            ExpectedField {
                name: "_seal",
                ty: None,
            },
        ],
    ));
    assert!(
        inventory_failures.is_empty(),
        "key-hygiene GUARD (exact inventory): a scanned shape/materialize body's \
         field inventory diverged from the pinned set. Each body must declare EXACTLY \
         the expected fields (no missing/extra) with the type-critical fields typed \
         exactly. A new/renamed field changes the key's content/identity surface and \
         must be reviewed (and, if sanctioned, added here). Failures:\n  {}",
        inventory_failures.join("\n  "),
    );

    let member_value_fields = split_struct_fields(&member_value_arm);

    // The member-arm SemanticNodeId allow-slot is pinned to the EXACT
    // field binding `node: MemberShapeNodeSubject`. A duplicate
    // `MemberShapeNodeSubject` field, or the sealed newtype attached to a
    // different field name, does NOT satisfy this — the second sanctioned
    // position is exactly `MemberValueNode.node`.
    let node_fields: Vec<&(String, String)> = member_value_fields
        .iter()
        .filter(|(n, _)| n == "node")
        .collect();
    assert_eq!(
        node_fields.len(),
        1,
        "key-hygiene GUARD: the `ShapeSubject::MemberValueNode` arm must declare EXACTLY \
         ONE `node` field; found {}: {member_value_arm}",
        node_fields.len(),
    );
    assert_eq!(
        normalize_ws(&node_fields[0].1),
        "MemberShapeNodeSubject",
        "key-hygiene GUARD: the `ShapeSubject::MemberValueNode.node` field must be typed \
         EXACTLY `MemberShapeNodeSubject` (the sealed newtype is the SECOND sanctioned \
         SemanticNodeId position); found `{}`",
        node_fields[0].1,
    );

    // `MemberShapeNodeSubject` is pinned GLOBALLY to exactly the
    // `MemberValueNode.node` field. The newtype hides its inner
    // `SemanticNodeId` from the bare-`SemanticNodeId` scan, so a MISPLACED
    // wrapper — `ShapeDemand { decoy: MemberShapeNodeSubject, .. }` or
    // `MaterializationCacheKey { rogue: MemberShapeNodeSubject, .. }` —
    // would otherwise PASS. Count the newtype across EVERY scanned body and
    // assert the SINGLE occurrence is the member arm's `node` field. The
    // `ShapeSubject` ENUM body already contains the member arm, so the four
    // top-level bodies counted are ShapeCacheKey + ShapeDemand +
    // MaterializationCacheKey + the whole ShapeSubject enum (which subsumes
    // the three arms) — no double-counting of the member arm.
    let member_subject_total = count_ident_tokens(&shape_cache_key, "MemberShapeNodeSubject")
        + count_ident_tokens(&shape_demand, "MemberShapeNodeSubject")
        + count_ident_tokens(&materialization_key, "MemberShapeNodeSubject")
        + count_ident_tokens(&shape_subject_enum, "MemberShapeNodeSubject");
    assert_eq!(
        member_subject_total, 1,
        "key-hygiene GUARD (global newtype pin): the sealed `MemberShapeNodeSubject` newtype \
         must appear EXACTLY ONCE across all scanned shape/materialize bodies — and that one \
         occurrence is `ShapeSubject::MemberValueNode.node`. A second occurrence (a \
         misplaced wrapper on `ShapeDemand` / `MaterializationCacheKey` etc.) hides a \
         `SemanticNodeId` behind the newtype and \
         re-opens the graph-instance ordinal key the content-free identities removed \
         (R6). Found {member_subject_total} occurrence(s).",
    );

    let mut violations = Vec::new();
    // Shape keys carry NO args list; the materialize key carries EXACTLY
    // ONE sanctioned `Arc<[SemanticNodeId]>` args list, ONLY under the
    // field name `normalized_type_args`.
    violations.extend(shape_materialize_key_violations(
        "ShapeCacheKey",
        &shape_cache_key,
        None,
    ));
    violations.extend(shape_materialize_key_violations(
        "ShapeDemand",
        &shape_demand,
        None,
    ));
    violations.extend(shape_materialize_key_violations(
        "ShapeSubject::MemberValueNode",
        &member_value_arm,
        None,
    ));
    violations.extend(shape_materialize_key_violations(
        "ShapeSubject::SyntheticBinding",
        &synthetic_binding_arm,
        None,
    ));
    violations.extend(shape_materialize_key_violations(
        "MaterializationCacheKey",
        &materialization_key,
        Some("normalized_type_args"),
    ));

    // ALSO scan the WHOLE `ShapeSubject` enum body with the
    // forbidden-marker predicate, so any bare `SemanticNodeId` /
    // content-version marker on a NEW or unreviewed arm is caught even
    // before the closed-inventory assert is updated. (The enum carries no
    // args list, hence `None`.) Note: the bare-`SemanticNodeId` count over
    // the whole enum body is 0 today — the only `SemanticNodeId` lives
    // inside the sealed `MemberShapeNodeSubject` newtype, which spells the
    // newtype name, not `SemanticNodeId`, in the enum body.
    violations.extend(shape_materialize_key_violations(
        "ShapeSubject (whole enum body)",
        &shape_subject_enum,
        None,
    ));

    // The two sanctioned positions are PINNED: the materialize key carries
    // exactly its one `normalized_type_args` `Arc<[SemanticNodeId]>` args
    // list, and the `MemberValueNode` arm keys on the sealed
    // `MemberShapeNodeSubject` newtype (asserted by field above). A future
    // edit that drops either representation, or adds a third, trips here.
    let materialization_fields = split_struct_fields(&materialization_key);
    assert_eq!(
        materialization_fields
            .iter()
            .filter(|(n, ty)| n == "normalized_type_args" && type_is_args_list(ty))
            .count(),
        1,
        "key-hygiene GUARD: `MaterializationCacheKey` must carry EXACTLY ONE sanctioned \
         `normalized_type_args: Arc<[SemanticNodeId]>` args position; the allow-list is exact",
    );

    assert!(
        violations.is_empty(),
        "key-hygiene GUARD VIOLATION — a shape/materialize derived-`Hash` cache key carries an \
         unsanctioned content/version marker or an unsanctioned `SemanticNodeId` position. A \
         shape/materialize key must be content-free; the ONLY sanctioned `SemanticNodeId` \
         positions are `MaterializationCacheKey.normalized_type_args` (`Arc<[SemanticNodeId]>`) \
         and the sealed `MemberShapeNodeSubject` newtype keying \
         `ShapeSubject::MemberValueNode.node`. This is a RECORDED SOURCE SCANNER \
         (per the binding neutral design ruling: a recorded scanner, not structural \
         enforcement). Violations:\n  {}",
        violations.join("\n  "),
    );
}

/// Self-test: the closed `ShapeSubject` variant-inventory assertion
/// provably FAILS on a hypothetical 3rd arm carrying a bare
/// `SemanticNodeId`, and provably PASSES on the sanctioned two. Without
/// this, the closed-inventory and whole-enum-body scan could be a stub.
#[test]
fn shape_subject_closed_inventory_self_test() {
    // The real (sanctioned) shape: exactly the two known arms, in order.
    let sanctioned_enum = "MemberValueNode { scope: Arc<str>, node: MemberShapeNodeSubject }, \
                           SyntheticBinding { id: SyntheticBindingId, _seal: ConstructionSeal },";
    assert_eq!(
        enum_variant_names(sanctioned_enum)
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        SANCTIONED_SHAPE_SUBJECT_VARIANTS,
        "self-test: the sanctioned two-arm enum must enumerate to exactly the \
         sanctioned variant set",
    );

    // A hypothetical 3rd arm carrying a bare `SemanticNodeId` — the closed
    // inventory must NO LONGER match the sanctioned set (FAILS the guard).
    let with_rogue_arm = "MemberValueNode { scope: Arc<str>, node: MemberShapeNodeSubject }, \
                          SyntheticBinding { id: SyntheticBindingId, _seal: ConstructionSeal }, \
                          Other { node: SemanticNodeId },";
    let rogue_variants = enum_variant_names(with_rogue_arm);
    assert_ne!(
        rogue_variants
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        SANCTIONED_SHAPE_SUBJECT_VARIANTS,
        "self-test: a 3rd `Other` arm must break the closed inventory equality",
    );
    assert!(
        rogue_variants.iter().any(|v| v == "Other"),
        "self-test: the inventory enumerator must surface the rogue `Other` arm",
    );

    // And the WHOLE-enum-body forbidden/bare-SemanticNodeId scan catches
    // the rogue arm's bare `SemanticNodeId` directly (independent of the
    // closed-inventory assert) — `Other { node: SemanticNodeId }` carries a
    // bare `SemanticNodeId` token.
    let v = shape_materialize_key_violations("ShapeSubject (rogue)", with_rogue_arm, None);
    assert!(
        v.iter().any(|m| m.contains("BARE `SemanticNodeId`")),
        "self-test: the whole-enum-body scan must flag the rogue 3rd arm's bare \
         `SemanticNodeId`; got {v:?}",
    );

    // The sanctioned enum body itself carries NO bare `SemanticNodeId`
    // (the only `SemanticNodeId` is hidden inside `MemberShapeNodeSubject`,
    // which spells the newtype name, not `SemanticNodeId`).
    assert!(
        shape_materialize_key_violations("ShapeSubject (sanctioned)", sanctioned_enum, None)
            .is_empty(),
        "self-test: the sanctioned `ShapeSubject` enum body must be clean (the \
         `SemanticNodeId` is sealed behind `MemberShapeNodeSubject`)",
    );

    // An ATTRIBUTED 3rd arm must STILL surface its name, so the
    // closed-inventory assert FAILS on it. Reading only a leading
    // identifier would let `#[cfg(test)] Other { .. }` (starting with
    // `#`) produce no name, so a content-free attributed arm would stay
    // invisible to the inventory. With attribute-stripping the arm's name
    // surfaces and the inventory no longer matches.
    let with_attributed_arm = "MemberValueNode { scope: Arc<str>, node: MemberShapeNodeSubject }, \
                               SyntheticBinding { id: SyntheticBindingId, _seal: ConstructionSeal }, \
                               #[cfg(test)] Other { scope: Arc<str> },";
    let attributed_variants = enum_variant_names(with_attributed_arm);
    assert!(
        attributed_variants.iter().any(|v| v == "Other"),
        "self-test (attributed-arm inventory): the inventory enumerator must surface the \
         name of an ATTRIBUTED arm (`#[cfg(test)] Other`); attribute-stripping is required \
         so an attributed, content-free arm cannot stay invisible. Found: {attributed_variants:?}",
    );
    assert_ne!(
        attributed_variants
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        SANCTIONED_SHAPE_SUBJECT_VARIANTS,
        "self-test (attributed-arm inventory): an attributed 3rd arm must break the \
         closed-inventory equality (the guard FAILS on it)",
    );
    // A MULTI-LINE / MULTI-attribute form must also surface the name.
    let multiline_attr_arm = "MemberValueNode { scope: Arc<str>, node: MemberShapeNodeSubject }, \
                              SyntheticBinding { id: SyntheticBindingId, _seal: ConstructionSeal }, \
                              #[cfg(test)]\n    #[doc = \"x\"]\n    Other { scope: Arc<str> },";
    assert!(
        enum_variant_names(multiline_attr_arm)
            .iter()
            .any(|v| v == "Other"),
        "self-test (attributed-arm inventory): a multi-line, multi-attribute arm must still \
         surface its name after attribute-stripping",
    );
}

/// Self-test: the member-arm allow-slot is pinned to the EXACT field
/// binding `node: MemberShapeNodeSubject`. A duplicate
/// `MemberShapeNodeSubject` field, or the newtype attached to a different
/// field name, or the newtype appearing in a DIFFERENT key body, must NOT
/// satisfy "the member arm keys on the sealed newtype". This proves the
/// member exception is field-pinned, not "appears somewhere".
#[test]
fn member_arm_sealed_newtype_is_field_pinned_self_test() {
    // (a) The sanctioned member arm: exactly `node: MemberShapeNodeSubject`.
    let ok_arm = "scope: Arc<str>, node: MemberShapeNodeSubject";
    let fields = split_struct_fields(ok_arm);
    let node: Vec<&(String, String)> = fields.iter().filter(|(n, _)| n == "node").collect();
    assert_eq!(node.len(), 1);
    assert_eq!(normalize_ws(&node[0].1), "MemberShapeNodeSubject");

    // (b) A DUPLICATE `MemberShapeNodeSubject` field — the count-1
    //     invariant the guard asserts must reject this.
    let dup_arm = "scope: Arc<str>, node: MemberShapeNodeSubject, \
                   decoy: MemberShapeNodeSubject";
    let dup_count = split_struct_fields(dup_arm)
        .iter()
        .filter(|(_, ty)| normalize_ws(ty) == "MemberShapeNodeSubject")
        .count();
    assert_eq!(
        dup_count, 2,
        "self-test: a duplicate `MemberShapeNodeSubject` field must be detectable as a \
         count > 1 — the guard's count-1 assert then rejects it",
    );

    // (c) The newtype attached to a DIFFERENT field name does NOT satisfy
    //     `node: MemberShapeNodeSubject`.
    let wrong_name_arm = "scope: Arc<str>, other_slot: MemberShapeNodeSubject";
    let has_node_binding = split_struct_fields(wrong_name_arm)
        .iter()
        .any(|(n, ty)| n == "node" && normalize_ws(ty) == "MemberShapeNodeSubject");
    assert!(
        !has_node_binding,
        "self-test: the sealed newtype on a non-`node` field must NOT register as the \
         sanctioned `node: MemberShapeNodeSubject` binding",
    );

    // (d) `MemberShapeNodeSubject` mentioned in a DIFFERENT key body (e.g.
    //     the materialize key) does NOT satisfy the member-arm requirement
    //     — the guard reads the field from the EXTRACTED member arm only,
    //     never the whole file. We model that here: the materialize body
    //     declares no `node: MemberShapeNodeSubject` field.
    let materialize_body = "decl: ResolvedDeclSlotIdentity, \
                            normalized_type_args: Arc<[SemanticNodeId]>, \
                            resolve_env_hash: HashValue";
    assert!(
        !split_struct_fields(materialize_body)
            .iter()
            .any(|(n, ty)| n == "node" && normalize_ws(ty) == "MemberShapeNodeSubject"),
        "self-test: the materialize body must not carry a `node: MemberShapeNodeSubject` \
         field — proving the member-arm requirement is satisfied only by the member \
         arm's own extracted body",
    );
}

/// Self-test: `MemberShapeNodeSubject` is pinned GLOBALLY to exactly
/// ONE occurrence across the scanned shape/materialize bodies. The newtype
/// hides its inner `SemanticNodeId` from the bare-`SemanticNodeId` scan, so
/// a MISPLACED wrapper in a NON-member body must be caught by the
/// global-count pin — not silently accepted. This models the guard's
/// global `count_ident_tokens(.., "MemberShapeNodeSubject")` sum.
#[test]
fn member_shape_node_subject_global_single_occurrence_self_test() {
    // The legitimate shape: the newtype appears ONCE, in the member arm
    // (inside the whole `ShapeSubject` enum body), and nowhere in
    // ShapeCacheKey / ShapeDemand / MaterializationCacheKey.
    let shape_cache_key = "subject: ShapeSubject, demand: ShapeDemand";
    let shape_demand = "path: Arc<[PathSegment]>, terminal_context: ProjectionReductionContext, \
                        key_filter: KeyFilter, surface: PublishedSurfaceKind";
    let materialization_key = "decl: ResolvedDeclSlotIdentity, projection_path: RouteDemand, \
                               scope_axis: MaterializationScope, projection_mode: ProjectionMode, \
                               normalized_type_args: Arc<[SemanticNodeId]>, \
                               resolve_env_hash: HashValue";
    let shape_subject_enum = "MemberValueNode { scope: Arc<str>, node: MemberShapeNodeSubject }, \
                              SyntheticBinding { id: SyntheticBindingId, _seal: ConstructionSeal }";
    let legit_total = count_ident_tokens(shape_cache_key, "MemberShapeNodeSubject")
        + count_ident_tokens(shape_demand, "MemberShapeNodeSubject")
        + count_ident_tokens(materialization_key, "MemberShapeNodeSubject")
        + count_ident_tokens(shape_subject_enum, "MemberShapeNodeSubject");
    assert_eq!(
        legit_total, 1,
        "self-test (global newtype pin): in the legitimate shape the sealed newtype appears \
         EXACTLY ONCE (the member arm's `node` slot); got {legit_total}",
    );

    // A MISPLACED wrapper on `ShapeDemand` (a non-member body) — the
    // global count now reaches 2, so the guard's `== 1` pin FAILS. This is
    // the exact false-negative the global newtype pin closes: the
    // bare-`SemanticNodeId` scan never sees the inner ordinal because it is
    // sealed behind the newtype.
    let rogue_shape_demand = "path: Arc<[PathSegment]>, decoy: MemberShapeNodeSubject, \
                              terminal_context: ProjectionReductionContext, \
                              key_filter: KeyFilter, surface: PublishedSurfaceKind";
    let rogue_total = count_ident_tokens(shape_cache_key, "MemberShapeNodeSubject")
        + count_ident_tokens(rogue_shape_demand, "MemberShapeNodeSubject")
        + count_ident_tokens(materialization_key, "MemberShapeNodeSubject")
        + count_ident_tokens(shape_subject_enum, "MemberShapeNodeSubject");
    assert_eq!(
        rogue_total, 2,
        "self-test (global newtype pin): a misplaced `MemberShapeNodeSubject` on `ShapeDemand` \
         must push the global count to 2 — the guard's `== 1` pin then rejects it; got {rogue_total}",
    );

    // And a misplaced wrapper on `MaterializationCacheKey` / `ShapeCacheKey`
    // is equally caught — the count is summed over ALL the bodies.
    let rogue_mat = "decl: ResolvedDeclSlotIdentity, rogue: MemberShapeNodeSubject, \
                     normalized_type_args: Arc<[SemanticNodeId]>, resolve_env_hash: HashValue";
    let rogue_mat_total = count_ident_tokens(shape_cache_key, "MemberShapeNodeSubject")
        + count_ident_tokens(shape_demand, "MemberShapeNodeSubject")
        + count_ident_tokens(rogue_mat, "MemberShapeNodeSubject")
        + count_ident_tokens(shape_subject_enum, "MemberShapeNodeSubject");
    assert_eq!(
        rogue_mat_total, 2,
        "self-test (global newtype pin): a misplaced `MemberShapeNodeSubject` on \
         `MaterializationCacheKey` must also push the global count to 2; got {rogue_mat_total}",
    );
}

/// Self-test: `strip_visibility` strips ONLY the real `pub` visibility
/// token (`pub`, `pub `, `pub(crate)`, `pub(in path)`), never a bare `pub`
/// prefix of a longer identifier. A bare `strip_prefix` would mis-credit
/// `pubnode` → `node` and `pubnormalized_type_args` →
/// `normalized_type_args`, letting those rogue identifiers SATISFY the
/// exact allow-list (a false negative). The visibility-token boundary
/// keeps them DISTINCT.
#[test]
fn strip_visibility_only_strips_the_pub_token_self_test() {
    // The real visibility-token forms ARE stripped.
    assert_eq!(strip_visibility("pub node"), "node");
    assert_eq!(strip_visibility("pub(crate) node"), "node");
    assert_eq!(strip_visibility("pub(in crate::x) node"), "node");
    assert_eq!(strip_visibility("pub"), "");
    // No visibility at all — returned unchanged.
    assert_eq!(strip_visibility("node"), "node");

    // A `pub`-PREFIXED longer identifier is NOT a visibility token —
    // it must be returned WHOLE, not truncated to the suffix.
    assert_eq!(
        strip_visibility("pubnode"),
        "pubnode",
        "visibility-token boundary: `pubnode` is a distinct identifier, not `pub`-qualified `node`",
    );
    assert_eq!(
        strip_visibility("pubnormalized_type_args"),
        "pubnormalized_type_args",
        "visibility-token boundary: `pubnormalized_type_args` is a distinct identifier, not \
         `pub`-qualified `normalized_type_args`",
    );

    // And therefore the rogue identifiers are NOT credited as the
    // sanctioned fields. Drive the actual field parser the guard uses: a
    // `pubnode: MemberShapeNodeSubject` field must NOT register as the
    // sanctioned `node` binding, and a `pubnormalized_type_args:
    // Arc<[SemanticNodeId]>` field must NOT satisfy the sanctioned
    // `normalized_type_args` args slot (so the violation pass flags it).
    let rogue_node_arm = "scope: Arc<str>, pubnode: MemberShapeNodeSubject";
    assert!(
        !split_struct_fields(rogue_node_arm)
            .iter()
            .any(|(n, _)| n == "node"),
        "visibility-token boundary: `pubnode` must NOT be parsed as the sanctioned `node` field",
    );
    let rogue_args_body = "decl: ResolvedDeclSlotIdentity, \
                           pubnormalized_type_args: Arc<[SemanticNodeId]>, \
                           resolve_env_hash: HashValue";
    assert!(
        !split_struct_fields(rogue_args_body)
            .iter()
            .any(|(n, _)| n == "normalized_type_args"),
        "visibility-token boundary: `pubnormalized_type_args` must NOT be parsed as the \
         sanctioned `normalized_type_args` field",
    );
    // The args-slot allow-list (pinned to the EXACT name `normalized_type_args`)
    // therefore REJECTS the rogue field: it carries an `Arc<[SemanticNodeId]>`
    // typed field whose name is NOT the sanctioned name.
    let v = shape_materialize_key_violations(
        "MaterializationCacheKey",
        rogue_args_body,
        Some("normalized_type_args"),
    );
    assert!(
        v.iter().any(
            |m| m.contains("UNSANCTIONED `Arc<[SemanticNodeId]>` args field")
                && m.contains("pubnormalized_type_args")
        ),
        "visibility-token boundary: a `pubnormalized_type_args` args field must be REJECTED as \
         unsanctioned (it is not the sanctioned `normalized_type_args` name); got {v:?}",
    );
}

/// Self-test: `exact_field_inventory_failures` enforces the EXACT
/// field-name inventory (no missing, no extra) and the type-critical field
/// types. A body with an UNEXPECTED extra field FAILS — proving the
/// inventory check does not lean on the violation pass. This also exercises
/// the global newtype pin: an unexpected `MemberShapeNodeSubject`-typed
/// field in a non-member body is caught by the exact inventory.
#[test]
fn exact_field_inventory_discriminates_self_test() {
    let mat_expected = &[
        ExpectedField {
            name: "decl",
            ty: None,
        },
        ExpectedField {
            name: "normalized_type_args",
            ty: Some("Arc<[SemanticNodeId]>"),
        },
        ExpectedField {
            name: "resolve_env_hash",
            ty: None,
        },
    ];

    // (a) The exact inventory PASSES on the matching body.
    let ok = "decl: ResolvedDeclSlotIdentity, normalized_type_args: Arc<[SemanticNodeId]>, \
              resolve_env_hash: HashValue";
    assert!(
        exact_field_inventory_failures("MaterializationCacheKey", ok, mat_expected).is_empty(),
        "self-test (exact inventory): the exact inventory must accept the matching body",
    );

    // (b) An UNEXPECTED extra field FAILS (no reliance on the violation
    //     pass) — here a stray `MemberShapeNodeSubject`-typed field, which
    //     also exercises the global newtype pin.
    let extra = "decl: ResolvedDeclSlotIdentity, normalized_type_args: Arc<[SemanticNodeId]>, \
                 resolve_env_hash: HashValue, rogue: MemberShapeNodeSubject";
    let f = exact_field_inventory_failures("MaterializationCacheKey", extra, mat_expected);
    assert!(
        f.iter().any(|m| m.contains("UNEXPECTED field `rogue`")),
        "self-test (exact inventory): an unexpected extra field must FAIL the exact inventory; got {f:?}",
    );

    // (c) A MISSING expected field FAILS.
    let missing = "decl: ResolvedDeclSlotIdentity, resolve_env_hash: HashValue";
    let f = exact_field_inventory_failures("MaterializationCacheKey", missing, mat_expected);
    assert!(
        f.iter()
            .any(|m| m.contains("MISSING expected field `normalized_type_args`")),
        "self-test (exact inventory): a missing expected field must FAIL the exact inventory; got {f:?}",
    );

    // (d) A type-critical field with the WRONG type FAILS (the args slot
    //     retyped to a bare `SemanticNodeId` subject).
    let wrong_type = "decl: ResolvedDeclSlotIdentity, normalized_type_args: SemanticNodeId, \
                      resolve_env_hash: HashValue";
    let f = exact_field_inventory_failures("MaterializationCacheKey", wrong_type, mat_expected);
    assert!(
        f.iter()
            .any(|m| m.contains("`normalized_type_args`") && m.contains("Arc<[SemanticNodeId]>")),
        "self-test (exact inventory): a type-critical field with the wrong type must FAIL; got {f:?}",
    );

    // (e) The member arm: `node` is type-critical (`MemberShapeNodeSubject`).
    //     The sanctioned arm PASSES; the newtype on the wrong-typed `node`
    //     (e.g. a bare `SemanticNodeId`) FAILS.
    let member_expected = &[
        ExpectedField {
            name: "scope",
            ty: None,
        },
        ExpectedField {
            name: "node",
            ty: Some("MemberShapeNodeSubject"),
        },
    ];
    assert!(
        exact_field_inventory_failures(
            "ShapeSubject::MemberValueNode",
            "scope: Arc<str>, node: MemberShapeNodeSubject",
            member_expected,
        )
        .is_empty(),
        "self-test (exact inventory): the sanctioned member arm must pass the exact inventory",
    );
    let bare_node = "scope: Arc<str>, node: SemanticNodeId";
    let f =
        exact_field_inventory_failures("ShapeSubject::MemberValueNode", bare_node, member_expected);
    assert!(
        f.iter()
            .any(|m| m.contains("`node`") && m.contains("MemberShapeNodeSubject")),
        "self-test (exact inventory): a `node` field NOT typed `MemberShapeNodeSubject` (a bare \
         `SemanticNodeId` subject) must FAIL the type-critical check; got {f:?}",
    );
}
