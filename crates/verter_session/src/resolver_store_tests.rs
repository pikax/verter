//! Tests for [`crate::resolver_store::HostStoreView`] — the
//! session-overlay-aware fact validator: `validates` arms,
//! untracked-file optimistic accept, and the route-surface digest.

use crate::resolver_store::HostStoreView;
use rustc_hash::FxHashMap;

use crate::resolver_core::StoreView;

/// Files loaded as dependencies DURING resolution (after the store view
/// snapshot was taken) are not tracked in `whole_hashes`. The validated
/// cache must accept facts for these untracked files — otherwise every
/// access to a dependency falls through to the expensive permissive path.
#[test]
fn validates_accepts_untracked_file_whole_hash() {
    let view = HostStoreView::with_whole_hashes_for_tests(FxHashMap::from_iter([(
        "/src/Accordion.vue".to_string(),
        [1u8; 16],
    )]));

    // Tracked file with matching hash — should validate.
    assert!(
        view.validates(&crate::resolver_core::FactVersionRef::FileWholeHash {
            canonical_id: "/src/Accordion.vue".to_string(),
            hash: [1u8; 16],
        })
    );

    // Tracked file with mismatching hash — should reject.
    assert!(
        !view.validates(&crate::resolver_core::FactVersionRef::FileWholeHash {
            canonical_id: "/src/Accordion.vue".to_string(),
            hash: [2u8; 16],
        })
    );

    // Untracked dependency file — should accept (loaded after view snapshot).
    assert!(
        view.validates(&crate::resolver_core::FactVersionRef::FileWholeHash {
            canonical_id: "/node_modules/vue/dist/vue.d.mts".to_string(),
            hash: [42u8; 16],
        }),
        "untracked dependency files should be accepted by the store view"
    );
}

/// DerivedFactHash::DirectSource for untracked files should be accepted
/// (same as FileWholeHash — it's a content-hash alias). Non-DirectSource
/// derived facts for untracked files should NOT be accepted — they are
/// invalidation signals (import routes, etc.) that must be explicitly
/// tracked to participate in validation.
#[test]
fn validates_derived_fact_hash_semantics() {
    let view = HostStoreView::with_whole_hashes_for_tests(FxHashMap::default());

    // DirectSource for untracked file — should accept (content-hash alias).
    assert!(
        view.validates(&crate::resolver_core::FactVersionRef::DerivedFactHash {
            canonical_id: "/node_modules/reka-ui/dist/index.d.ts".to_string(),
            kind: crate::resolver_core::DerivedFactKind::DirectSource,
            hash: [99u8; 16],
        }),
        "DirectSource for untracked file should be accepted"
    );

    // Route for untracked file — should NOT accept (invalidation signal).
    assert!(
        !view.validates(&crate::resolver_core::FactVersionRef::DerivedFactHash {
            canonical_id: "/node_modules/reka-ui/dist/index.d.ts".to_string(),
            kind: crate::resolver_core::DerivedFactKind::Route,
            hash: [99u8; 16],
        }),
        "Route derived fact for untracked file should NOT be accepted"
    );
}

/// Concurrent generations of the same key are distinguished by
/// per-candidate fact validation against the candidate's own
/// `fact_dep_signature` (see
/// `crates/verter_session/src/resolver_core/mod.rs`
/// `ValidatedFactCache` substrate). For untracked files, the
/// primary `validates` path accepts the cached hash because the
/// candidate was admitted from current workspace content.
#[test]
fn primary_validates_accepts_untracked_file_whole_hash() {
    let view = HostStoreView::with_whole_hashes_for_tests(FxHashMap::from_iter([(
        "/src/tracked.ts".to_string(),
        [1u8; 16],
    )]));

    // Tracked file — matches.
    assert!(
        view.validates(&crate::resolver_core::FactVersionRef::FileWholeHash {
            canonical_id: "/src/tracked.ts".to_string(),
            hash: [1u8; 16],
        })
    );

    // Tracked file — mismatched hash rejected.
    assert!(
        !view.validates(&crate::resolver_core::FactVersionRef::FileWholeHash {
            canonical_id: "/src/tracked.ts".to_string(),
            hash: [2u8; 16],
        })
    );

    // Untracked file — accepted (multi-candidate
    // substrate relies on the candidate's own `fact_dep_signature`
    // to discriminate concurrent generations).
    assert!(
        view.validates(&crate::resolver_core::FactVersionRef::FileWholeHash {
            canonical_id: "/node_modules/vue/dist/vue.d.mts".to_string(),
            hash: [42u8; 16],
        }),
        "untracked files are accepted by primary validation in the multi-candidate substrate"
    );
}

// ── FileSourceEnv strict source-env validation ──

mod file_source_env_validation {
    use super::*;
    use crate::file_artifact_store::FileArtifactKey;
    use crate::locator_identity::ParseEnvHash;
    use crate::resolver_core::FactVersionRef;
    use crate::resolver_store::SourceEnvIdentity;

    const CONTRIB: &str = "/contrib.d.ts";
    const CONTRIB_HASH: [u8; 16] = [5u8; 16];

    fn live_identity() -> SourceEnvIdentity {
        SourceEnvIdentity {
            parse_env_hash: ParseEnvHash::from_env_hash([3u8; 16]),
            parse_key: crate::build_toolchain_fingerprint::parse_key_for_test(CONTRIB, 2),
            file_language_id: FileArtifactKey::synthetic_file_language_for_test(CONTRIB),
        }
    }

    fn recorded_fact(
        canonical: &str,
        env_byte: u8,
        parse_marker: u8,
        language_of: &str,
    ) -> FactVersionRef {
        FactVersionRef::FileSourceEnv {
            canonical_id: canonical.to_string(),
            parse_env_hash: ParseEnvHash::from_env_hash([env_byte; 16]),
            parse_key: crate::build_toolchain_fingerprint::parse_key_for_test(
                language_of,
                parse_marker,
            ),
            file_language_id: FileArtifactKey::synthetic_file_language_for_test(language_of),
        }
    }

    /// The recorded fact matching the [`live_identity`] plant.
    fn matching_fact() -> FactVersionRef {
        recorded_fact(CONTRIB, 3, 2, CONTRIB)
    }

    fn whole_hash_fact() -> FactVersionRef {
        FactVersionRef::FileWholeHash {
            canonical_id: CONTRIB.to_string(),
            hash: CONTRIB_HASH,
        }
    }

    /// A view tracking the contributor's whole hash AND its live
    /// source-env identity.
    fn planted_view() -> HostStoreView {
        HostStoreView::with_source_env_snapshot_for_tests(
            FxHashMap::from_iter([(CONTRIB.to_string(), CONTRIB_HASH)]),
            FxHashMap::from_iter([(CONTRIB.to_string(), live_identity())]),
            std::collections::HashSet::new(),
        )
    }

    #[test]
    fn file_source_env_validates_matching_live_identity() {
        let view = planted_view();
        assert!(
            view.validates(&matching_fact()),
            "a recorded source-env identity equal to the view-current identity must validate"
        );
    }

    #[test]
    fn file_source_env_rejects_canonical_id_mismatch() {
        let view = planted_view();
        assert!(
            !view.validates(&recorded_fact("/other.d.ts", 3, 2, CONTRIB)),
            "a contributor canonical the view has no source-env identity for must reject"
        );
    }

    #[test]
    fn file_source_env_rejects_parse_env_hash_mismatch_with_valid_whole_hash() {
        let view = planted_view();
        let stale = recorded_fact(CONTRIB, 4, 2, CONTRIB);
        // Isolation: the sibling whole-hash fact still validates —
        // rejection comes from the source-env branch alone.
        assert!(
            view.validates(&whole_hash_fact()),
            "sanity: the contributor FileWholeHash must still validate under this view"
        );
        assert!(
            !view.validates(&stale),
            "a recorded parse_env_hash differing from the view-current identity must reject"
        );
        assert!(
            !view.validates_fact_signature(&[whole_hash_fact(), stale]),
            "the full signature must reject on the source-env mismatch even though the \
             whole-hash fact still matches"
        );
    }

    #[test]
    fn file_source_env_rejects_parse_key_mismatch() {
        let view = planted_view();
        assert!(
            !view.validates(&recorded_fact(CONTRIB, 3, 7, CONTRIB)),
            "a recorded parse_key differing from the view-current identity must reject"
        );
    }

    #[test]
    fn file_source_env_rejects_file_language_mismatch() {
        let view = planted_view();
        assert!(
            !view.validates(&recorded_fact(CONTRIB, 3, 2, "/contrib.vue")),
            "a recorded file_language_id differing from the view-current identity must reject"
        );
    }

    #[test]
    fn file_source_env_rejects_missing_identity_even_when_whole_hash_matches() {
        // Whole hash tracked and matching, but NO source-env identity
        // for the contributor: the strict branch must reject (never
        // the optimistic untracked-accept the whole-hash arm applies).
        let view = HostStoreView::with_source_env_snapshot_for_tests(
            FxHashMap::from_iter([(CONTRIB.to_string(), CONTRIB_HASH)]),
            FxHashMap::default(),
            std::collections::HashSet::new(),
        );
        assert!(
            view.validates(&whole_hash_fact()),
            "sanity: the contributor FileWholeHash must validate under this view"
        );
        assert!(
            !view.validates(&matching_fact()),
            "a missing view-current source-env identity must reject strictly"
        );
    }

    #[test]
    fn file_source_env_rejects_tombstoned_canonical() {
        // Tombstoned wins even over a matching planted identity.
        let view = HostStoreView::with_source_env_snapshot_for_tests(
            FxHashMap::from_iter([(CONTRIB.to_string(), CONTRIB_HASH)]),
            FxHashMap::from_iter([(CONTRIB.to_string(), live_identity())]),
            std::collections::HashSet::from_iter([CONTRIB.to_string()]),
        );
        assert!(
            !view.validates(&matching_fact()),
            "a tombstoned contributor canonical must reject its source-env fact"
        );
    }

    #[test]
    fn file_source_env_rejects_untracked_canonical() {
        let view = HostStoreView::with_source_env_snapshot_for_tests(
            FxHashMap::default(),
            FxHashMap::default(),
            std::collections::HashSet::new(),
        );
        assert!(
            !view.validates(&matching_fact()),
            "an untracked contributor canonical must reject strictly, never optimistically accept"
        );
    }
}

// ── `hash_route_surface` — purity pin + per-state memoization ──

mod route_surface_hash {
    use crate::resolver_core::shallow_file_state::{
        ExportTarget, ImportTarget, ShallowFileState, WildcardReexport,
    };
    use rustc_hash::{FxHashMap, FxHashSet};
    use std::sync::Arc;
    use verter_semantic::analysis::Hash16;

    fn parsed_state(
        canonical: &str,
        source: &str,
        file_language: crate::types::FileLanguage,
    ) -> Arc<ShallowFileState> {
        let host = crate::VerterHost::new_standalone(crate::types::HostConfig::default());
        let _ = host
            .upsert(crate::types::UpsertRequest {
                canonical_id: Some(canonical.to_string()),
                input_id: canonical.to_string(),
                source: Arc::from(source),
                file_language,
                aliases: Vec::new(),
            })
            .expect("upsert parsed route fixture");
        host.ensure_indexed_ready(canonical)
            .expect("materialize parsed route fixture")
            .shallow_state
            .clone()
    }

    fn parsed_script_state(source: &str) -> Arc<ShallowFileState> {
        parsed_state(
            "/src/route.ts",
            source,
            crate::types::FileLanguage::script_ts(),
        )
    }

    /// A routing surface exercising every dimension `hash_route_surface`
    /// digests: local exports, a named reexport (authored specifier,
    /// original name, type-only flag), a wildcard edge, and an import
    /// target. All of it PARSE domain — no resolved canonical.
    pub(super) fn routed_state(whole_hash: Hash16) -> ShallowFileState {
        let exports = FxHashMap::from_iter([
            (
                "Local".to_string(),
                ExportTarget::Local {
                    owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                    symbol_name: "Local".to_string(),
                },
            ),
            (
                "Renamed".to_string(),
                ExportTarget::Reexport {
                    source_specifier: "./dep".to_string(),
                    original_name: "Orig".to_string(),
                    is_type: true,
                },
            ),
        ]);
        let wildcard_reexports = vec![WildcardReexport {
            source_specifier: "./barrel".to_string(),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
        }];
        let import_locals = FxHashSet::from_iter(["Dep".to_string()]);
        let import_targets = FxHashMap::from_iter([(
            "Dep".to_string(),
            ImportTarget {
                source_specifier: "./dep".to_string(),
                imported_name: "Dep".to_string(),
                is_namespace: false,
            },
        )]);
        ShallowFileState::routing_tables_only_for_test(
            whole_hash,
            exports,
            wildcard_reexports,
            import_locals,
            import_targets,
            Arc::new(
                verter_parser::utils::oxc::script::route_inventory::ScriptRouteInventory::default(),
            ),
        )
    }

    /// The hash is a pure function of the routing surface: an
    /// INDEPENDENTLY CONSTRUCTED identical state hashes identically, and
    /// any surface move (reexport retarget, content-hash move) moves it.
    #[test]
    fn matches_independent_recomputation_and_discriminates_surface_moves() {
        let state = routed_state([7u8; 16]);
        let independent = routed_state([7u8; 16]);
        assert_eq!(
            crate::resolver_store::hash_route_surface(&state),
            crate::resolver_store::hash_route_surface(&independent),
            "identical routing surfaces must hash identically",
        );

        // Negative: an authored-SPECIFIER move moves the hash — the
        // surface is digested WITH its routing shape, never just export
        // names. The mutation happens strictly BEFORE the state's first
        // hash (the construction-time mutation window).
        let mut respecified = routed_state([7u8; 16]);
        respecified.exports.insert(
            "Renamed".to_string(),
            ExportTarget::Reexport {
                source_specifier: "./other".to_string(),
                original_name: "Orig".to_string(),
                is_type: true,
            },
        );
        assert_ne!(
            crate::resolver_store::hash_route_surface(&state),
            crate::resolver_store::hash_route_surface(&respecified),
            "an authored reexport-specifier move must move the route-surface hash",
        );

        // Negative: the type-only flag participates too.
        let mut value_only = routed_state([7u8; 16]);
        value_only.exports.insert(
            "Renamed".to_string(),
            ExportTarget::Reexport {
                source_specifier: "./dep".to_string(),
                original_name: "Orig".to_string(),
                is_type: false,
            },
        );
        assert_ne!(
            crate::resolver_store::hash_route_surface(&state),
            crate::resolver_store::hash_route_surface(&value_only),
            "the type-only flag must move the route-surface hash",
        );

        // Negative: the owner content hash participates.
        let content_moved = routed_state([8u8; 16]);
        assert_ne!(
            crate::resolver_store::hash_route_surface(&state),
            crate::resolver_store::hash_route_surface(&content_moved),
            "a whole-hash move must move the route-surface hash",
        );
    }

    /// The parse-owned route-interface fact is stable across content-only
    /// edits while the legacy route digest remains content-sensitive. Every
    /// authored coordinate a resolver can inspect still moves the new fact.
    #[test]
    fn syntactic_route_interface_discriminates_only_authored_route_coordinates() {
        let state = routed_state([7u8; 16]);
        let content_only = routed_state([8u8; 16]);
        let expected = crate::resolver_store::syntactic_route_interface_hash(&state);

        assert_eq!(
            expected,
            crate::resolver_store::syntactic_route_interface_hash(&content_only),
            "a template/comment/body-only edit changes whole_hash but not the authored route interface",
        );
        assert_ne!(
            crate::resolver_store::hash_route_surface(&state),
            crate::resolver_store::hash_route_surface(&content_only),
            "the legacy Route digest must remain content-sensitive",
        );

        let mut mutations: Vec<(&str, ShallowFileState)> = Vec::new();

        let mut local_backing = routed_state([7u8; 16]);
        local_backing.exports.insert(
            "Local".to_string(),
            ExportTarget::Local {
                owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                symbol_name: "OtherBacking".to_string(),
            },
        );
        mutations.push(("local backing symbol", local_backing));

        let mut exported_name = routed_state([7u8; 16]);
        let renamed = exported_name
            .exports
            .remove("Renamed")
            .expect("fixture reexport");
        exported_name
            .exports
            .insert("OtherExport".to_string(), renamed);
        mutations.push(("exported name", exported_name));

        let mut reexport_specifier = routed_state([7u8; 16]);
        reexport_specifier.exports.insert(
            "Renamed".to_string(),
            ExportTarget::Reexport {
                source_specifier: "./other".to_string(),
                original_name: "Orig".to_string(),
                is_type: true,
            },
        );
        mutations.push(("reexport specifier", reexport_specifier));

        let mut reexport_original = routed_state([7u8; 16]);
        reexport_original.exports.insert(
            "Renamed".to_string(),
            ExportTarget::Reexport {
                source_specifier: "./dep".to_string(),
                original_name: "Different".to_string(),
                is_type: true,
            },
        );
        mutations.push(("reexport original name", reexport_original));

        let mut reexport_capability = routed_state([7u8; 16]);
        reexport_capability.exports.insert(
            "Renamed".to_string(),
            ExportTarget::Reexport {
                source_specifier: "./dep".to_string(),
                original_name: "Orig".to_string(),
                is_type: false,
            },
        );
        mutations.push(("reexport type/value capability", reexport_capability));

        for (coordinate, mutated) in mutations {
            assert_ne!(
                expected,
                crate::resolver_store::syntactic_route_interface_hash(&mutated),
                "authored route coordinate must move the fact: {coordinate}",
            );
        }
    }

    #[test]
    fn syntactic_route_interface_v2_covers_exact_parsed_route_geometry() {
        let cases = [
            (
                "import form",
                "import d from './dep'; export { d };",
                "import * as d from './dep'; export { d };",
            ),
            (
                "import capability",
                "import type { D as d } from './dep'; export type { d };",
                "import { D as d } from './dep'; export { d };",
            ),
            (
                "bindingless import source",
                "import './left';",
                "import './right';",
            ),
            (
                "direct reexport capability",
                "export type { D as Public } from './dep';",
                "export { D as Public } from './dep';",
            ),
            (
                "wildcard capability",
                "export type * from './dep';",
                "export * from './dep';",
            ),
            (
                "wildcard namespace variant",
                "export * from './dep';",
                "export * as Public from './dep';",
            ),
            (
                "local export capability",
                "type Public = {}; export type { Public };",
                "const Public = 1; export { Public };",
            ),
            (
                "export assignment",
                "const Public = 1;",
                "const Public = 1; export = Public;",
            ),
            (
                "parser route order",
                "import './first'; import './second';",
                "import './second'; import './first';",
            ),
        ];
        for (coordinate, before, after) in cases {
            let before = parsed_script_state(before);
            let after = parsed_script_state(after);
            assert_ne!(
                crate::resolver_store::syntactic_route_interface_hash(before.as_ref()),
                crate::resolver_store::syntactic_route_interface_hash(after.as_ref()),
                "parsed authored route coordinate must move the v2 fact: {coordinate}",
            );
        }

        let authored_in_regular = parsed_state(
            "/src/owners.vue",
            "<script lang=\"ts\">import { X as routed } from './dep'; export { routed };</script><script setup lang=\"ts\">const setup = 1;</script><template />",
            crate::types::FileLanguage::vue(),
        );
        let authored_in_setup = parsed_state(
            "/src/owners.vue",
            "<script lang=\"ts\">const regular = 1; export { routed };</script><script setup lang=\"ts\">import { X as routed } from './dep';</script><template />",
            crate::types::FileLanguage::vue(),
        );
        assert_ne!(
            crate::resolver_store::syntactic_route_interface_hash(authored_in_regular.as_ref()),
            crate::resolver_store::syntactic_route_interface_hash(authored_in_setup.as_ref()),
            "owner-qualified route geometry must distinguish the SFC script owner",
        );

        let carrier = parsed_state(
            "/src/default.vue",
            "<script setup lang=\"ts\">const value = 1;</script><template>{{ value }}</template>",
            crate::types::FileLanguage::vue(),
        );
        assert!(
            carrier.exports.contains_key("default"),
            "fixture must contain the effective synthesized component default",
        );
        let mut without_effective_default = carrier.as_ref().clone();
        without_effective_default.exports.remove("default");
        assert_ne!(
            crate::resolver_store::syntactic_route_interface_hash(carrier.as_ref()),
            crate::resolver_store::syntactic_route_interface_hash(&without_effective_default),
            "the effective synthesized export surface participates in the v2 fact",
        );
    }

    #[test]
    fn syntactic_route_interface_v2_excludes_body_spans_content_and_resolution_state() {
        let before =
            parsed_script_state("export function routed() { const body = 1; return body; }");
        let after = parsed_script_state(
            "// leading span movement\nexport function routed() { const body = 2; return body + 1; }",
        );
        assert_ne!(before.whole_hash, after.whole_hash);
        assert_eq!(
            crate::resolver_store::syntactic_route_interface_hash(before.as_ref()),
            crate::resolver_store::syntactic_route_interface_hash(after.as_ref()),
            "comments, spans, declaration bodies, and whole content are outside the authored route interface",
        );
        assert_ne!(
            crate::resolver_store::hash_route_surface(before.as_ref()),
            crate::resolver_store::hash_route_surface(after.as_ref()),
            "the legacy Route digest still composes the whole-content hash",
        );

        let host = crate::VerterHost::new_standalone(crate::types::HostConfig::default());
        let canonical = "/src/independent.ts";
        let _ = host
            .upsert(crate::types::UpsertRequest {
                canonical_id: Some(canonical.to_string()),
                input_id: canonical.to_string(),
                source: Arc::from("export { D as Public } from './dep';"),
                file_language: crate::types::FileLanguage::script_ts(),
                aliases: Vec::new(),
            })
            .expect("upsert independent route fixture");
        let state = host
            .ensure_indexed_ready(canonical)
            .expect("materialize independent route fixture")
            .shallow_state
            .clone();
        let expected = crate::resolver_store::syntactic_route_interface_hash(state.as_ref());
        host.set_exact_resolutions(
            canonical,
            vec![verter_workspace::ExactResolution {
                specifier: "./dep".to_string(),
                phase: verter_semantic::resolver_core::ResolvePhase::CodegenBlocker,
                kind: verter_semantic::resolver_core::ResolveRequestKind::TypeImport,
                resolved_canonical_id: Some("/src/other.ts".to_string()),
                possible_canonical_ids: vec!["/src/other.ts".to_string()],
            }],
        );
        host.configure_projects(vec![verter_workspace::ide_project_config(
            "/src".to_string(),
            "/src".to_string(),
            Some("/src/tsconfig.json".to_string()),
        )]);
        assert_eq!(
            crate::resolver_store::syntactic_route_interface_hash(state.as_ref()),
            expected,
            "resolved canonicals and project/environment generations are not parse-route identity",
        );
    }
}

// The memo test lives in the same module as the purity pin so both read
// the same `routed_state` fixture.
mod route_surface_hash_memo {
    use super::route_surface_hash::routed_state;
    use crate::resolver_core::shallow_file_state::ExportTarget;

    /// The memo populates on the state's FIRST hash and every later call
    /// returns the identical value; a CLONE starts with an EMPTY memo
    /// (fresh `OnceLock`) so its independent recomputation agrees on an
    /// unmutated clone and re-digests the clone's OWN surface after a
    /// clone-side mutation (never the donor's stale digest).
    #[test]
    fn memoizes_per_state_and_resets_on_clone() {
        let state = routed_state([9u8; 16]);
        assert!(
            state.route_surface_hash_memo().get().is_none(),
            "memo must start unpopulated",
        );
        let first = crate::resolver_store::hash_route_surface(&state);
        assert_eq!(
            state.route_surface_hash_memo().get(),
            Some(first),
            "first hash must populate the memo with the computed digest",
        );
        assert_eq!(
            first,
            crate::resolver_store::hash_route_surface(&state),
            "the memoized (second) call must return the identical value",
        );

        // Init path: a clone resets the memo and recomputes fresh — the
        // recomputation must agree with the donor's digest.
        let cloned = state.clone();
        assert!(
            cloned.route_surface_hash_memo().get().is_none(),
            "a clone must start with an EMPTY memo, not the donor's cached digest",
        );
        assert_eq!(
            first,
            crate::resolver_store::hash_route_surface(&cloned),
            "an unmutated clone's fresh computation must equal the donor's digest",
        );

        // Clone-then-mutate: the reset means a mutated clone hashes its
        // OWN surface. Carrying the donor's populated memo across the
        // clone would serve a stale digest here.
        let mut mutated = state.clone();
        mutated.exports.insert(
            "Extra".to_string(),
            ExportTarget::Local {
                owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                symbol_name: "Extra".to_string(),
            },
        );
        assert_ne!(
            first,
            crate::resolver_store::hash_route_surface(&mutated),
            "a mutated clone must hash its own surface, not the donor's cached digest",
        );
    }
}

mod syntactic_route_interface_fact {
    use super::route_surface_hash::routed_state;
    use crate::resolver_core::{FactReadSetFinalise, FactVersionRef, StoreView};
    use crate::types::{FileLanguage, HostConfig, UpsertRequest};
    use crate::VerterHost;
    use std::sync::Arc;
    use verter_semantic::facts::{FactKey, FactLane};

    fn upsert(host: &VerterHost, source: &str) {
        let _ = host
            .upsert(UpsertRequest {
                canonical_id: None,
                input_id: "/src/route.ts".to_string(),
                source: Arc::from(source),
                file_language: FileLanguage::script_ts(),
                aliases: Vec::new(),
            })
            .expect("upsert route fixture");
    }

    fn upsert_carrier(
        host: &VerterHost,
        canonical: &str,
        source: &str,
        file_language: FileLanguage,
    ) {
        let _ = host
            .upsert(UpsertRequest {
                canonical_id: Some(canonical.to_string()),
                input_id: canonical.to_string(),
                source: Arc::from(source),
                file_language,
                aliases: Vec::new(),
            })
            .expect("upsert carrier route fixture");
    }

    #[test]
    fn parse_emission_uses_the_single_syntactic_route_interface_hash() {
        let state = routed_state([11u8; 16]);
        let indexed = Arc::new(
            crate::project_type_store::IndexedReady::new_for_test_with_state(
                state.whole_hash,
                Arc::new(state.clone()),
                Arc::from(""),
                Arc::from(""),
            ),
        );
        let artifacts = crate::file_artifact_store::FileArtifacts::with_indexed(indexed);
        let fact = artifacts
            .facts
            .lookup(&FactKey::SyntacticRouteInterface)
            .expect("parse-time emission must publish the authored route-interface fact");
        let expected = crate::resolver_store::syntactic_route_interface_hash(&state);
        assert_eq!(fact.semantic_hash, expected);
        assert_eq!(fact.display_hash, expected);
    }

    #[test]
    fn shared_indexed_serve_observes_parse_route_interface_without_legacy_route() {
        let host = VerterHost::new_standalone(HostConfig::default());
        upsert(&host, "export const value = 1;\n");

        let (_, read_set) = host
            .with_fact_tracer(verter_workspace::AggregateBasisSeed::Unvouched, || {
                host.ensure_indexed_ready("/src/route.ts")
            });
        let FactReadSetFinalise::Ok(facts) = read_set.finalise() else {
            panic!("a published indexed serve must finalize its route-only read set");
        };
        assert!(facts.iter().any(|fact| matches!(
            fact,
            FactVersionRef::Parse(parse)
                if parse.canonical_id == "/src/route.ts"
                    && parse.key == FactKey::SyntacticRouteInterface
                    && parse.lane == FactLane::Semantic
        )));
        assert!(!facts.iter().any(|fact| matches!(
            fact,
            FactVersionRef::DerivedFactHash {
                canonical_id,
                kind: crate::resolver_core::DerivedFactKind::Route,
                ..
            } if canonical_id == "/src/route.ts"
        )));
    }

    #[test]
    fn store_view_accepts_content_only_move_and_rejects_authored_route_move() {
        let host = VerterHost::new_standalone(HostConfig::default());
        upsert(&host, "export const value = 1;\n");
        let indexed = host
            .ensure_indexed_ready("/src/route.ts")
            .expect("initial indexed route fixture");
        let parse = host
            .syntactic_route_interface_fact_for_indexed("/src/route.ts", &indexed)
            .expect("exact emitted fact for initial artifact");
        let fact = FactVersionRef::Parse(parse);
        let initial = host
            .resolver_store_view_read()
            .current()
            .expect("initial current store view");
        assert!(initial.view().validates(&fact));

        upsert(&host, "export const value = 2; // body-only\n");
        host.ensure_indexed_ready("/src/route.ts")
            .expect("content-only replacement indexed");
        let content_only = host
            .resolver_store_view_read()
            .current()
            .expect("content-only current store view");
        assert!(
            content_only.view().validates(&fact),
            "content-only edits preserve the authored route-interface fact",
        );

        upsert(&host, "export const renamed = 2;\n");
        host.ensure_indexed_ready("/src/route.ts")
            .expect("authored route replacement indexed");
        let route_moved = host
            .resolver_store_view_read()
            .current()
            .expect("route-moved current store view");
        assert!(
            !route_moved.view().validates(&fact),
            "an authored route-interface edit must invalidate the old parse fact",
        );
    }

    #[test]
    fn carrier_template_only_commit_immediately_publishes_current_route_interface_fact() {
        let host = VerterHost::new_standalone(HostConfig::default());
        let canonical = "/src/route.vue";
        upsert_carrier(
            &host,
            canonical,
            "<script setup lang=\"ts\">\nexport const value = 1;\n</script>\n<template><p>one</p></template>\n",
            FileLanguage::vue(),
        );
        let indexed = host
            .ensure_indexed_ready(canonical)
            .expect("initial indexed carrier route fixture");
        let fact = FactVersionRef::Parse(
            host.syntactic_route_interface_fact_for_indexed(canonical, &indexed)
                .expect("exact emitted fact for initial carrier artifact"),
        );

        upsert_carrier(
            &host,
            canonical,
            "<script setup lang=\"ts\">\nexport const value = 1;\n</script>\n<template><p>two</p></template>\n",
            FileLanguage::vue(),
        );
        let published = host
            .current_content_pinned_artifacts(canonical)
            .expect("template-only commit publishes fresh current-content artifacts");
        assert_ne!(published.indexed.whole_hash, indexed.whole_hash);
        assert!(
            !Arc::ptr_eq(&published.indexed, &indexed),
            "the eager producer must materialize fresh new-content geometry, not reuse the old IndexedReady Arc",
        );
        assert!(published.indexed.raw_source.contains("<p>two</p>"));
        let current = host
            .resolver_store_view_read()
            .current()
            .expect("post-commit current store view");
        assert!(
            current.view().validates(&fact),
            "the committed template-only edit must publish current-content FileFacts before request-time validation",
        );
    }

    #[test]
    fn svelte_markup_only_commit_immediately_publishes_current_route_interface_fact() {
        let host = VerterHost::new_standalone(HostConfig::default());
        let canonical = "/src/route.svelte";
        upsert_carrier(
            &host,
            canonical,
            "<script lang=\"ts\">\nexport const value = 1;\n</script>\n<p>one</p>\n",
            FileLanguage::svelte(),
        );
        let indexed = host
            .ensure_indexed_ready(canonical)
            .expect("initial indexed Svelte carrier route fixture");
        let fact = FactVersionRef::Parse(
            host.syntactic_route_interface_fact_for_indexed(canonical, &indexed)
                .expect("exact emitted fact for initial Svelte carrier artifact"),
        );

        upsert_carrier(
            &host,
            canonical,
            "<script lang=\"ts\">\nexport const value = 1;\n</script>\n<p>two</p>\n",
            FileLanguage::svelte(),
        );
        let published = host
            .current_content_pinned_artifacts(canonical)
            .expect("markup-only commit publishes fresh current-content artifacts");
        assert_ne!(published.indexed.whole_hash, indexed.whole_hash);
        assert!(
            !Arc::ptr_eq(&published.indexed, &indexed),
            "the Svelte eager producer must materialize fresh new-content geometry",
        );
        assert!(published.indexed.raw_source.contains("<p>two</p>"));
        let current = host
            .resolver_store_view_read()
            .current()
            .expect("post-commit current store view");
        assert!(
            current.view().validates(&fact),
            "the committed markup-only edit must publish current-content FileFacts before request-time validation",
        );
    }

    #[test]
    fn eager_carrier_route_publication_rejects_changed_route_language_and_script_envelope() {
        struct Case {
            name: &'static str,
            source: &'static str,
            language: fn() -> FileLanguage,
        }

        let cases = [
            Case {
                name: "authored route",
                source: "<script setup lang=\"ts\">\nexport const renamed = 1;\n</script>\n<template><p>two</p></template>\n",
                language: FileLanguage::vue,
            },
            Case {
                name: "carrier language",
                source: "<script lang=\"ts\">\nexport const value = 1;\n</script>\n<p>two</p>\n",
                language: FileLanguage::svelte,
            },
            Case {
                name: "script language",
                source: "<script setup lang=\"js\">\nexport const value = 1;\n</script>\n<template><p>two</p></template>\n",
                language: FileLanguage::vue,
            },
            Case {
                name: "script envelope",
                source: "<script lang=\"ts\">\nexport const moduleValue = 1;\n</script>\n<script setup lang=\"ts\">\nexport const value = 1;\n</script>\n<template><p>two</p></template>\n",
                language: FileLanguage::vue,
            },
        ];

        for case in cases {
            let host = VerterHost::new_standalone(HostConfig::default());
            let canonical = "/src/route.carrier";
            upsert_carrier(
                &host,
                canonical,
                "<script setup lang=\"ts\">\nexport const value = 1;\n</script>\n<template><p>one</p></template>\n",
                FileLanguage::vue(),
            );
            let indexed = host
                .ensure_indexed_ready(canonical)
                .expect("initial indexed carrier route fixture");
            let fact = FactVersionRef::Parse(
                host.syntactic_route_interface_fact_for_indexed(canonical, &indexed)
                    .expect("exact emitted fact for initial carrier artifact"),
            );

            upsert_carrier(&host, canonical, case.source, (case.language)());

            assert!(
                host.current_content_pinned_artifacts(canonical).is_none(),
                "{} mutation must not eagerly publish a route-interface artifact",
                case.name,
            );
            let current = host
                .resolver_store_view_read()
                .current()
                .expect("post-mutation current store view");
            assert!(
                !current.view().validates(&fact),
                "{} mutation must invalidate the old route-interface fact",
                case.name,
            );
        }
    }

    #[test]
    fn eager_carrier_route_publication_refuses_a_superseded_materialization() {
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

        let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
        let canonical = "/src/route.vue";
        upsert_carrier(
            &host,
            canonical,
            "<script setup lang=\"ts\">\nexport const value = 1;\n</script>\n<template><p>one</p></template>\n",
            FileLanguage::vue(),
        );
        host.ensure_indexed_ready(canonical)
            .expect("initial indexed carrier route fixture");

        let parked = Arc::new(AtomicBool::new(false));
        let release = Arc::new(AtomicBool::new(false));
        let calls = Arc::new(AtomicUsize::new(0));
        {
            let parked = Arc::clone(&parked);
            let release = Arc::clone(&release);
            let calls = Arc::clone(&calls);
            *host.materialize_seam_hook.lock() = Some(Arc::new(move || {
                if calls.fetch_add(1, Ordering::SeqCst) == 1 {
                    parked.store(true, Ordering::SeqCst);
                    while !release.load(Ordering::SeqCst) {
                        std::thread::yield_now();
                    }
                }
            }));
        }

        let flight = {
            let host = Arc::clone(&host);
            std::thread::spawn(move || {
                upsert_carrier(
                    &host,
                    canonical,
                    "<script setup lang=\"ts\">\nexport const value = 1;\n</script>\n<template><p>two</p></template>\n",
                    FileLanguage::vue(),
                );
            })
        };
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !parked.load(Ordering::SeqCst) {
            assert!(
                std::time::Instant::now() < deadline,
                "eager materialization did not reach its pre-publication seam",
            );
            std::thread::yield_now();
        }
        host.project_type_store.bump_project_generation();
        release.store(true, Ordering::SeqCst);
        flight.join().expect("upsert flight joins");
        *host.materialize_seam_hook.lock() = None;

        assert!(
            host.current_content_pinned_artifacts(canonical).is_none(),
            "a materialization superseded at the pre-publication fence must not publish",
        );
    }
}
