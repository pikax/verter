//! The `ResolvedImportFactsKey` `resolve_env_hash` producer/validator
//! asymmetry — reproduced, then pinned.
//!
//! ## The asymmetry
//!
//! The producer keys on the LIVE PER-CANONICAL env
//! (`VerterHost::host_view_env_hashes_for`: resolve the canonical's owning
//! project, read that project's env array, else the workspace default).
//! The validator composed its `ResolvedImportFactsKey` from the
//! view-captured WORKSPACE-LEVEL
//! `project_env_root.env_hashes.resolve_env_hash`.
//!
//! ## Why this one IS reachable
//!
//! Its `parse_env_hash` sibling is unreachable because
//! `IdeProjectConfig::parse_env_hash` never reads `&self` — every project's
//! value is byte-identical (pinned by `parse_env_asymmetry_tests`).
//! `resolve_env_hash` is the opposite: it folds `workspace_aliases`,
//! `compiler_options` (`base_url`, `paths`, …) and `references`, all
//! per-project, while the workspace default is computed from a synthetic
//! config that has none of them. So any project carrying real resolution
//! configuration — the normal case for a monorepo package — composes a
//! `resolve_env_hash` that differs from the workspace default, and the two
//! sides keyed DIFFERENT slots for the same fact.
//!
//! ## What was measured, and what was NOT
//!
//! Measured, at the STORE: the producer admitted under the per-canonical key
//! and a lookup under the workspace-default key found NOTHING. That is a real
//! key divergence and it is what `validator_finds_the_bundle_…` pins.
//!
//! NOT measured, because it does not exist today: a consumer-visible
//! regression. The Semantic arm this validator serves is **production-dead** —
//! every `ResolveImportsFactRef::Semantic` CONSTRUCTION in the tree is inside a
//! test module or a test file; the only production construction of
//! `FactVersionRef::ResolveImports(..)` is the `Resolution` arm, which never
//! composes this key; and `SessionView::resolved_import_facts` has no
//! production callers. So no production consumer records one of these facts
//! yet, and the divergence causes neither staleness nor wasted work at present.
//!
//! This is therefore a LATENT, pre-arming correctness fix: it is repaired now,
//! while the arm is inert and the repair is cheap and verifiable, so that the
//! first production consumer to record one of these facts does not inherit a
//! silent whole-slot miss for every non-default project. The claim here is
//! deliberately no stronger than the evidence — the errata's ADOPT-NOW was
//! honest about having only source dataflow, and this adds a store-level
//! reproduction of the mechanism, not a measured production regression.
//!
//! ## The fix
//!
//! The validator composes both env dimensions through the CAPTURED
//! per-canonical accessors (`ProjectEnvRoot::parse_env_hash_for` /
//! `resolve_env_hash_for`), which mirror the producer's project resolution
//! while reading the captured published root so the answer cannot drift
//! under a live re-publication.
//!
//! ## Mutation recipes — both applicable to the landed tree as written
//!
//! (A) COMPOSER. In `HostStoreView::resolved_import_facts_key_for`, replace
//! the line `resolve_env_hash: env_root.resolve_env_hash_for(canonical),`
//! with `resolve_env_hash: env_root.env_hashes.resolve_env_hash,`.
//! ⇒ ALL THREE of `validator_finds_the_bundle_…`,
//! `production_validator_accepts_a_fact_…` and
//! `session_view_reader_finds_the_bundle_…` FAIL — the third because the
//! derived `SessionView` reader routes through this same composer.
//!
//! (B) VALIDATOR CALL SITE. In
//! `HostStoreView::validates_resolve_imports_domain_for_content_hash`,
//! replace the single line
//! `let key = self.resolved_import_facts_key_for(canonical_id.as_str(), content_hash);`
//! with an inline `ResolvedImportFactsKey` whose `resolve_env_hash` is
//! `self.snapshot.roots.project_env_root.env_hashes.resolve_env_hash`.
//! ⇒ `production_validator_accepts_a_fact_…` FAILS and the other two stay
//! GREEN — which is exactly why that test exists: the store-level pair does
//! not execute the validator, so this mutation is invisible to them.
//!
//! Both were applied to the landed tree, confirmed present / unique / new in
//! the source, run, and reverted by inverse edit.

use std::sync::Arc;

use verter_workspace::resolver::{IdeProjectConfig, WorkspaceAlias};

use crate::{HostConfig, UpsertRequest, VerterHost};

fn upsert(host: &VerterHost, path: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(path.to_string()),
            input_id: path.to_string(),
            source: Arc::from(source),
            file_language: crate::LanguageRegistry::global()
                .classify_static(path)
                .static_resolution(),
            aliases: Vec::new(),
        })
        .unwrap_or_else(|e| panic!("upsert {path} failed: {e:?}"));
}

/// A two-project workspace whose project `A` carries real resolution
/// configuration (a workspace alias plus a `baseUrl`) and whose project `B`
/// carries none. `B` therefore matches the synthetic workspace default and
/// `A` does not — the exact shape that separates the two keying paths.
fn configure_two_projects(host: &VerterHost) {
    let mut a = IdeProjectConfig::new(
        "/ws/a".to_string(),
        "/ws".to_string(),
        Some("/ws/a/tsconfig.json".to_string()),
    );
    a.workspace_aliases.push(WorkspaceAlias {
        find: "@a".to_string(),
        replacement: "/ws/a/src".to_string(),
    });
    a.compiler_options.base_url = Some("/ws/a".to_string());
    let b = IdeProjectConfig::new(
        "/ws/b".to_string(),
        "/ws".to_string(),
        Some("/ws/b/tsconfig.json".to_string()),
    );
    host.configure_projects(vec![a, b]);
}

/// The reachability precondition, asserted independently of the cache: a
/// canonical owned by a project with real resolution config composes a
/// `resolve_env_hash` that DIFFERS from the workspace default, while a
/// canonical owned by a bare project matches it.
///
/// This is what makes `resolve_env_hash` unlike its `parse_env_hash`
/// sibling, and it is the load-bearing premise of the end-to-end test
/// below. If a future change ever made `resolve_env_hash` project
/// independent, this test fails FIRST and explains that the end-to-end
/// guard below has gone vacuous.
#[test]
fn resolve_env_hash_is_project_dependent_so_the_key_asymmetry_is_reachable() {
    let host = VerterHost::new_standalone(HostConfig::default());
    configure_two_projects(&host);
    upsert(&host, "/ws/a/one.ts", "export const a = 1;\n");
    upsert(&host, "/ws/b/two.ts", "export const b = 2;\n");

    let owned_by_a = host
        .host_view_env_hashes_for("/ws/a/one.ts")
        .resolve_env_hash;
    let owned_by_b = host
        .host_view_env_hashes_for("/ws/b/two.ts")
        .resolve_env_hash;
    // No owning project → the synthetic workspace-default array.
    let workspace_default = host
        .host_view_env_hashes_for("/outside/three.ts")
        .resolve_env_hash;

    assert_ne!(
        owned_by_a, workspace_default,
        "a project carrying real resolution config (aliases + baseUrl) MUST compose a \
         resolve_env_hash distinct from the synthetic workspace default — that gap is \
         precisely what the producer/validator key asymmetry falls into"
    );
    assert_eq!(
        owned_by_b, workspace_default,
        "a bare project must still match the workspace default, so the divergence above \
         is caused by A's resolution config and not by project identity as such"
    );
    assert_ne!(
        owned_by_a, owned_by_b,
        "the two projects must compose different resolve envs"
    );
}

/// END-TO-END: a bundle admitted by the producer for a canonical owned by a
/// NON-DEFAULT project must be findable by the validator's own key
/// composition.
///
/// Before the fix the producer admitted under A's per-canonical
/// `resolve_env_hash` while the validator composed the workspace default,
/// so this lookup missed every time and the consumer recomputed forever.
///
/// Discriminating: the lookup goes through the SAME captured accessor the
/// production validator uses, against a canonical whose owning project the
/// companion test above proves is genuinely off-default. A regression that
/// reverts the validator to the workspace-level field reds this immediately.
#[test]
fn validator_finds_the_bundle_a_non_default_project_owner_admitted() {
    let host = VerterHost::new_standalone(HostConfig::default());
    configure_two_projects(&host);
    upsert(&host, "/ws/a/dep.ts", "export const a = 1;\n");
    upsert(
        &host,
        "/ws/a/one.ts",
        "import { a } from './dep'\nexport const z = a;\n",
    );

    let mut routes = rustc_hash::FxHashMap::default();
    routes.insert(
        "./dep".to_string(),
        crate::types::DependencyResolution {
            specifier: "./dep".to_string(),
            resolved_canonical_id: Some("/ws/a/dep.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        },
    );
    assert!(
        host.admit_resolved_import_facts_for_owner("/ws/a/one.ts", &routes),
        "the producer must admit a bundle for the project-A owner"
    );

    let content_hash = host
        .current_or_read_whole_hash("/ws/a/one.ts")
        .expect("owner content hash");
    let view = host.resolver_store_view_read().into_owned_view();

    // The key the VALIDATOR composes, through the captured per-canonical
    // accessors it now uses.
    let validator_key = view.resolved_import_facts_key_for_tests("/ws/a/one.ts", content_hash);
    assert!(
        host.project_type_store()
            .resolved_import_facts()
            .get_if_valid(&validator_key, &view)
            .is_some(),
        "the validator's own key composition MUST find the bundle the producer just \
         admitted for a canonical owned by a project whose resolve env differs from the \
         workspace default. Composing the workspace-level resolve_env_hash here keys a \
         slot the producer never wrote, so every such bundle is unfindable and the \
         consumer recomputes on every pass."
    );

    // ANTI-VACUITY: the workspace-default composition — what the validator
    // used to do — genuinely misses. Without this the assertion above could
    // pass on a tree where the two compositions happened to coincide.
    let default_keyed = crate::resolved_import_facts::ResolvedImportFactsKey {
        canonical: Arc::from("/ws/a/one.ts"),
        content_hash,
        parse_env_hash: host.host_view_env_hashes_for("/ws/a/one.ts").parse_env_hash,
        resolve_env_hash: host
            .host_view_env_hashes_for("/outside/three.ts")
            .resolve_env_hash,
        resolver_version: crate::resolved_import_facts::RESOLVED_IMPORT_FACTS_RESOLVER_VERSION,
    };
    assert!(
        host.project_type_store()
            .resolved_import_facts()
            .get_if_valid(&default_keyed, &view)
            .is_none(),
        "the workspace-default-keyed lookup must MISS — if it hit, the two key \
         compositions coincide on this fixture and the guard above proves nothing"
    );
}

/// THE PRODUCTION VALIDATOR, driven end to end: `StoreView::validates` must
/// accept a Semantic `ResolveImports` fact for a canonical owned by a
/// non-default project.
///
/// This is the assertion that sits on the defect. The two tests above pin the
/// store-level key divergence and the composer; neither of them executes
/// `HostStoreView::validates_resolve_imports_domain_for_content_hash`, which
/// is where a consumer's recorded fact is actually judged. A revert of the
/// validator's key composition is invisible to them and visible here.
///
/// The fact is built from the bundle the production producer admitted — the
/// same `(specifier, binding, space, resolved_canonical, resolved_source_name)`
/// tuple and the same `semantic_hash` a consumer would have recorded — so the
/// only thing that can make `validates` return `false` is the validator
/// addressing a slot the producer never wrote.
///
/// Discriminating, and verified at BOTH mutation points: reverting
/// `resolved_import_facts_key_for` to the workspace-level `resolve_env_hash`,
/// and inlining that same workspace-level key directly inside
/// `validates_resolve_imports_domain_for_content_hash`, each turn this red.
#[test]
fn production_validator_accepts_a_fact_for_a_non_default_project_owner() {
    use verter_semantic::facts::registry::{FactKey, FactLane, InternedName, InternedSpecifier};

    use crate::resolver_core::{FactVersionRef, ResolveImportsFactRef, StoreView};

    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    configure_two_projects(&host);
    upsert(&host, "/ws/a/dep.ts", "export const a = 1;\n");
    upsert(
        &host,
        "/ws/a/one.ts",
        "import { a } from './dep'\nexport const z = a;\n",
    );

    // PRECONDITION: the owner is genuinely off-default, so this fixture sits
    // on the divergence rather than on a workspace-default canonical where
    // both compositions coincide.
    assert_ne!(
        host.host_view_env_hashes_for("/ws/a/one.ts")
            .resolve_env_hash,
        host.host_view_env_hashes_for("/outside/three.ts")
            .resolve_env_hash,
        "fixture invariant: the owner's project must carry a non-default resolve env"
    );

    // Production admission path.
    host.set_import_dependencies(
        "/ws/a/one.ts",
        vec![crate::types::DependencyResolution {
            specifier: "./dep".to_string(),
            resolved_canonical_id: Some("/ws/a/dep.ts".to_string()),
            possible_canonical_ids: vec!["/ws/a/dep.ts".to_string()],
        }],
    );

    let content_hash = host
        .current_or_read_whole_hash("/ws/a/one.ts")
        .expect("owner content hash");
    let admitted = host
        .project_type_store()
        .resolved_import_facts()
        .retained_bundle_for_tests(&crate::resolved_import_facts::ResolvedImportFactsKey {
            canonical: Arc::from("/ws/a/one.ts"),
            content_hash,
            parse_env_hash: host.host_view_env_hashes_for("/ws/a/one.ts").parse_env_hash,
            resolve_env_hash: host
                .host_view_env_hashes_for("/ws/a/one.ts")
                .resolve_env_hash,
            resolver_version: crate::resolved_import_facts::RESOLVED_IMPORT_FACTS_RESOLVER_VERSION,
        })
        .expect("the producer must admit a bundle under its own per-canonical key");
    let entry = admitted
        .import_clauses
        .iter()
        .find(|e| e.binding.as_ref() == "a")
        .expect("the `a` binding must be admitted into the payload");

    // Exactly the fact shape a consumer records after reading this bundle.
    let fact = FactVersionRef::ResolveImports(ResolveImportsFactRef::Semantic {
        canonical_id: "/ws/a/one.ts".to_string(),
        key: FactKey::ResolvedImportClause {
            specifier: InternedSpecifier::from(entry.specifier.as_ref()),
            binding: InternedName::from(entry.binding.as_ref()),
            space: entry.space,
            resolved_canonical: entry
                .resolved_canonical
                .as_ref()
                .map(Arc::clone)
                .expect("resolved canonical present"),
            resolved_source_name: InternedName::from(entry.resolved_source_name.as_ref()),
        },
        lane: FactLane::Semantic,
        expected_hash: entry.fact.semantic_hash,
    });

    let view = host.resolver_store_view_read().into_owned_view();
    assert!(
        view.validates(&fact),
        "the production validator MUST accept a recorded resolve-imports fact for a \
         canonical owned by a project whose resolve env differs from the workspace \
         default. Composing the workspace-level resolve_env_hash addresses a slot the \
         producer never wrote, so the lookup misses and every such fact is rejected."
    );
}

/// THE SECOND READER: `SessionView::resolved_import_facts` must also find the
/// bundle for a canonical owned by a non-default project.
///
/// The shared `session_view::resolved_import_facts_for_view` composed the same
/// key inline from the view's `env_hashes`, and every DERIVED constructor
/// (`HostView::new`, `HostViewRef::new`, `OverlaidView::new`,
/// `OverlaidViewRef::new`) sets that to the workspace-level bundle. So this
/// reader carried the identical asymmetry and had to be routed through the same
/// composer, not just the fact validator.
///
/// A caller that PINS an env bundle explicitly (`with_env_hashes` /
/// `with_overlay_hashes`) still addresses exactly its pinned slot — that
/// capability is what lets two views over one canonical read distinct
/// resolve-env entries, and the `resolved_import_facts_invariants` suite pins
/// it. This test covers the derived path only.
///
/// Discriminating: reverting `resolved_import_facts_for_view`'s `None` arm to
/// compose from a view-level `EnvHashes` bundle turns this red while the
/// explicit-pin invariants stay green.
#[test]
fn session_view_reader_finds_the_bundle_a_non_default_project_owner_admitted() {
    use crate::session_view::{HostView, SessionView};

    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    configure_two_projects(&host);
    upsert(&host, "/ws/a/dep.ts", "export const a = 1;\n");
    upsert(
        &host,
        "/ws/a/one.ts",
        "import { a } from './dep'\nexport const z = a;\n",
    );

    assert_ne!(
        host.host_view_env_hashes_for("/ws/a/one.ts")
            .resolve_env_hash,
        host.host_view_env_hashes_for("/outside/three.ts")
            .resolve_env_hash,
        "fixture invariant: the owner's project must carry a non-default resolve env"
    );

    host.set_import_dependencies(
        "/ws/a/one.ts",
        vec![crate::types::DependencyResolution {
            specifier: "./dep".to_string(),
            resolved_canonical_id: Some("/ws/a/dep.ts".to_string()),
            possible_canonical_ids: vec!["/ws/a/dep.ts".to_string()],
        }],
    );

    // The DERIVED constructor — no explicit env pin.
    let view = HostView::new(Arc::clone(&host));
    let payload = view.resolved_import_facts("/ws/a/one.ts").expect(
        "the derived SessionView reader MUST find the bundle the producer admitted for a \
         canonical owned by a project whose resolve env differs from the workspace \
         default — composing the view's workspace-level bundle as key dimensions \
         addresses a slot the per-canonical producer never wrote",
    );
    assert!(
        payload
            .import_clauses
            .iter()
            .any(|e| e.binding.as_ref() == "a"),
        "the bundle found must be the owner's real payload"
    );
}
