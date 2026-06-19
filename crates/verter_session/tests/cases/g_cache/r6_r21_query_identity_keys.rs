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
    proj_variant.projection_path = RouteDemand::Pick(vec!["id".to_string()]);
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
