//! Family-A `BinderIdentityFacts` discriminating tests — the
//! binder-identity substrate guard set.
//!
//! Covered contracts (one test per guard name registered for the
//! substrate):
//!
//! - Scope ids are cosmetic-edit stable and non-aliasing when a sibling
//!   scope is inserted above (no positional renumbering).
//! - Declaration-slot seeds are stable, symbol-space-scoped, and
//!   env-free (value vs type vs distinct owners occupy DISTINCT seeds).
//! - The demand-produced artifact warms on a cosmetic edit
//!   (`parse_stable_hash` invariant) and invalidates on a semantic edit
//!   (`ReadSetSignature` + key move).
//! - Overload-group / augmentation-contribution provenance is served
//!   from the artifact in authored order; no consumer re-walks raw
//!   `IndexedReady` for it (grep evidence).
//! - `binder_scope_id` enters context-sensitive query identity
//!   (`ResolveDecl`) as a content-free discriminator.
//! - Negative name lookup stays `ReturnOnly` (no corpus-completeness
//!   store in this substrate).

use std::sync::Arc;

use verter_semantic::analysis::type_eval::AugmentationScopeKind;
use verter_semantic::facts::FactKey;

use verter_session::binder_identity_facts::{
    negative_lookup_admission, BinderIdentityFacts, BinderIdentityFactsEntry,
};
use verter_session::for_tests::binder_identity_facts_get_or_compute_for_tests;
use verter_session::resolver_core::FactVersionRef;
use verter_session::semantic_query::admit::Admission;
use verter_session::semantic_query::{
    BinderScopeId, BinderScopeKind, DeclarationSlotSeed, ResolvedDeclSlotIdentity, ScopeId,
    SemanticQueryKey, SemanticSymbolSpace,
};
use verter_session::{FileLanguage, HostConfig, UpsertRequest, VerterHost};
use verter_type_expr::TopLevelOwnerId;
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

fn make_host() -> Arc<VerterHost> {
    let workspace: Arc<dyn WorkspaceAccess> =
        Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    Arc::new(VerterHost::new(HostConfig::default(), workspace))
}

fn upsert(host: &VerterHost, canonical: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(canonical.to_string()),
            input_id: canonical.to_string(),
            source: Arc::from(source),
            file_language: FileLanguage::script_ts(),
            aliases: vec![],
        })
        .expect("upsert must succeed");
}

fn make_host_with_file(canonical: &str, source: &str) -> Arc<VerterHost> {
    let host = make_host();
    upsert(&host, canonical, source);
    let _ = host.analyze_with_audit(canonical);
    host
}

fn produce(host: &VerterHost, canonical: &str) -> Arc<BinderIdentityFactsEntry> {
    binder_identity_facts_get_or_compute_for_tests(host, canonical)
        .expect("an analyzed file must produce a BinderIdentityFacts entry")
}

/// Map a scope tree into `(kind-tag, name)` → `BinderScopeId` so two
/// artifacts' scope ids can be compared per scope.
fn scope_id_map(
    facts: &BinderIdentityFacts,
) -> std::collections::BTreeMap<(u8, String), BinderScopeId> {
    facts
        .scopes
        .iter()
        .map(|record| {
            let key = match &record.kind {
                BinderScopeKind::File => (0u8, String::new()),
                BinderScopeKind::Namespace { qualified_name } => (1u8, qualified_name.to_string()),
                BinderScopeKind::AugmentationGlobal => (2u8, String::new()),
                BinderScopeKind::AugmentationModule { specifier } => (3u8, specifier.to_string()),
            };
            // The .ts fixtures here are single-owner (ordinary_file), so
            // the kind+name key is unique within one artifact.
            (key, record.id)
        })
        .collect()
}

fn seeds(facts: &BinderIdentityFacts) -> Vec<DeclarationSlotSeed> {
    facts.decl_slots.to_vec()
}

// ---------------------------------------------------------------------------
// Scope ids: cosmetic-stable + insertion-non-aliasing
// ---------------------------------------------------------------------------

/// Cosmetic edit (whitespace/comment churn) leaves scope ids AND seeds
/// unchanged; a sibling scope inserted ABOVE neither renumbers nor
/// aliases existing scope ids. This is the structural-identity
/// contract: ids are content-derived from the scope's own header
/// (owner + qualified name), never from a positional ordinal.
#[test]
fn binder_scope_ids_are_cosmetic_stable_and_insertion_non_aliasing() {
    const CANONICAL: &str = "/w/scopes.ts";
    const BASE: &str = "namespace Alpha { interface A1 {} }\nnamespace Beta { interface B1 {} }\n";
    // Cosmetic churn: comments + blank lines, identical declarations.
    const COSMETIC: &str =
        "// header comment\n\nnamespace Alpha {\n  /* inner */ interface A1 {}\n}\n\nnamespace Beta { interface B1 {} }\n// trailing\n";
    // A sibling scope inserted ABOVE Alpha — and sorting BEFORE it, so
    // a positional-ordinal derivation WOULD renumber Alpha/Beta (the
    // discriminating case: only a content-derived id survives).
    const INSERTED: &str = "namespace Aardvark { interface Z1 {} }\nnamespace Alpha { interface A1 {} }\nnamespace Beta { interface B1 {} }\n";

    let base_facts = produce(&make_host_with_file(CANONICAL, BASE), CANONICAL)
        .facts
        .clone();
    let cosmetic_facts = produce(&make_host_with_file(CANONICAL, COSMETIC), CANONICAL)
        .facts
        .clone();
    let inserted_facts = produce(&make_host_with_file(CANONICAL, INSERTED), CANONICAL)
        .facts
        .clone();

    // Cosmetic churn: every scope id AND every seed is unchanged.
    assert_eq!(
        scope_id_map(&base_facts),
        scope_id_map(&cosmetic_facts),
        "cosmetic edit must leave every binder scope id unchanged"
    );
    assert_eq!(
        seeds(&base_facts),
        seeds(&cosmetic_facts),
        "cosmetic edit must leave every declaration-slot seed unchanged"
    );

    // Sibling inserted above: existing scope ids are neither renumbered
    // nor aliased; the inserted scope gets a fresh, distinct id.
    let base_map = scope_id_map(&base_facts);
    let inserted_map = scope_id_map(&inserted_facts);
    for (key, base_id) in &base_map {
        assert_eq!(
            inserted_map.get(key),
            Some(base_id),
            "scope {key:?} must keep its id when a sibling is inserted above \
             (a positional ordinal would renumber it)"
        );
    }
    let zeta = inserted_map.get(&(1u8, "Aardvark".to_string()));
    assert!(
        zeta.is_some(),
        "the inserted sibling scope must appear with its own id"
    );
    assert!(
        !base_map.values().any(|id| Some(id) == zeta),
        "the inserted sibling's id must not alias any pre-existing scope id"
    );
    // The namespace seeds likewise survive the insertion verbatim.
    assert_eq!(
        seeds(&base_facts),
        seeds(&inserted_facts)
            .into_iter()
            .filter(|s| !s.merged_symbol_name.as_ref().starts_with("Aardvark"))
            .collect::<Vec<_>>(),
        "inserting a sibling scope must not disturb existing declaration-slot seeds"
    );
}

// ---------------------------------------------------------------------------
// Seeds: stable, symbol-space-scoped
// ---------------------------------------------------------------------------

/// A value and a type sharing a name occupy DISTINCT seeds
/// (`symbol_space` discriminates); a namespace sharing the name occupies
/// a third; same-name declarations in distinct owners occupy distinct
/// seeds (`owner` discriminates). Seeds carry NO env dimension (the type
/// has none by construction) and the artifact stores the seed, never the
/// env-bearing slot.
#[test]
fn declaration_slots_are_stable_symbol_space_scoped_facts() {
    const CANONICAL: &str = "/w/dual.ts";
    // `Foo` in THREE declaration spaces: type (interface) + value
    // (function) + namespace (`namespace Foo { … }`).
    let host = make_host_with_file(
        CANONICAL,
        "export interface Foo { x: string }\nexport function Foo(): void {}\nnamespace Foo { export interface Inner {} }\n",
    );
    let facts = produce(&host, CANONICAL).facts.clone();

    let type_seed = facts
        .decl_slot_seed(
            TopLevelOwnerId::ordinary_file(),
            "Foo",
            SemanticSymbolSpace::Type,
        )
        .expect("the type-space Foo seed must be recorded");
    let value_seed = facts
        .decl_slot_seed(
            TopLevelOwnerId::ordinary_file(),
            "Foo",
            SemanticSymbolSpace::Value,
        )
        .expect("the value-space Foo seed must be recorded");
    assert_ne!(
        type_seed, value_seed,
        "a value and a type sharing a name must occupy DISTINCT seeds"
    );
    assert_eq!(type_seed.defining_canonical.as_ref(), CANONICAL);
    assert_eq!(value_seed.defining_canonical.as_ref(), CANONICAL);

    // The namespace declaration introduces `Foo` in NAMESPACE
    // space: the artifact must record that seed too, DISTINCT from both
    // the type and the value seed.
    let namespace_seed = facts
        .decl_slot_seed(
            TopLevelOwnerId::ordinary_file(),
            "Foo",
            SemanticSymbolSpace::Namespace,
        )
        .expect("the namespace-space Foo seed must be recorded");
    assert_ne!(
        type_seed, namespace_seed,
        "a type and a namespace sharing a name must occupy DISTINCT seeds"
    );
    assert_ne!(
        value_seed, namespace_seed,
        "a value and a namespace sharing a name must occupy DISTINCT seeds"
    );
    assert_eq!(namespace_seed.defining_canonical.as_ref(), CANONICAL);
    assert_eq!(
        namespace_seed.merged_symbol_name.as_ref(),
        "Foo",
        "the namespace seed names the namespace itself (its members are dotted)"
    );

    // Same name, same space, DISTINCT owners → distinct seeds (the
    // owner field is mandatory: a .vue SFC's module vs instance scripts
    // may declare the same name).
    let module_seed = DeclarationSlotSeed::new(
        Arc::from(CANONICAL),
        TopLevelOwnerId::module(0),
        Arc::from("Foo"),
        SemanticSymbolSpace::Value,
    );
    let instance_seed = DeclarationSlotSeed::new(
        Arc::from(CANONICAL),
        TopLevelOwnerId::instance(0),
        Arc::from("Foo"),
        SemanticSymbolSpace::Value,
    );
    assert_ne!(
        module_seed, instance_seed,
        "same-name same-space declarations in distinct owners must occupy distinct seeds"
    );

    // The artifact stores the env-free SEED — the seed projected out of
    // an env-bearing slot (`ResolvedDeclSlotIdentity::seed`) must equal
    // the artifact's seed, and re-finalizing it with two different envs
    // must produce two distinct slots (env enters only the key, never
    // the artifact). Build two explicit-env slots via the public
    // explicit-env constructor and lift their sealed env tails.
    let env_a_slot = ResolvedDeclSlotIdentity::value_slot(
        Arc::from(CANONICAL),
        TopLevelOwnerId::ordinary_file(),
        Arc::from("Foo"),
        7,
        [0xA1u8; 16],
        [0xA2u8; 16],
    );
    let env_b_slot = ResolvedDeclSlotIdentity::value_slot(
        Arc::from(CANONICAL),
        TopLevelOwnerId::ordinary_file(),
        Arc::from("Foo"),
        9,
        [0xB1u8; 16],
        [0xB2u8; 16],
    );
    assert_eq!(
        env_a_slot.seed(),
        *value_seed,
        "an env-bearing slot's env-free projection must equal the artifact's seed"
    );
    let finalized_a = value_seed.clone().finalize(env_a_slot.env);
    let finalized_b = value_seed.clone().finalize(env_b_slot.env);
    assert_ne!(
        finalized_a, finalized_b,
        "two different envs over ONE seed must produce two distinct query identities"
    );
    assert_eq!(finalized_a.seed(), *value_seed);
    assert_eq!(finalized_b.seed(), *value_seed);
    assert_eq!(finalized_a, env_a_slot);
    assert_eq!(finalized_b, env_b_slot);
}

// ---------------------------------------------------------------------------
// Warm path: cosmetic warms, semantic edit invalidates
// ---------------------------------------------------------------------------

/// The demand-produced entry warms on a cosmetic edit (the
/// `parse_stable_hash` key is invariant AND the `ReadSetSignature`
/// still validates) and invalidates on a semantic edit (the key moves
/// AND the recorded parse-fact rail moves with it).
#[test]
fn binder_identity_facts_warm_on_cosmetic_edit_invalidate_on_semantic_edit() {
    const CANONICAL: &str = "/w/warm.ts";
    const V1: &str = "export interface Foo { a: string }\n";
    let host = make_host_with_file(CANONICAL, V1);

    let first = produce(&host, CANONICAL);
    let store = host.project_type_store().binder_identity_facts_store();
    assert_eq!(store.len(), 1, "the cold produce must admit one entry");

    // Cosmetic edit: comment + whitespace churn only.
    upsert(
        &host,
        CANONICAL,
        "// cosmetic churn\n\nexport interface Foo { a: string }\n\n",
    );
    let _ = host.analyze_with_audit(CANONICAL);
    let warm = produce(&host, CANONICAL);
    assert!(
        Arc::ptr_eq(&first, &warm),
        "a cosmetic edit must WARM the family-A entry (parse_stable_hash invariant, \
         ReadSetSignature still validates)"
    );
    assert_eq!(store.len(), 1, "a cosmetic edit must not grow the store");

    // Semantic edit: add a declaration — the decl skeleton moves.
    upsert(
        &host,
        CANONICAL,
        "// cosmetic churn\n\nexport interface Foo { a: string }\n\nexport function g(): void;\n",
    );
    let _ = host.analyze_with_audit(CANONICAL);
    let recomputed = produce(&host, CANONICAL);
    assert!(
        !Arc::ptr_eq(&first, &recomputed),
        "a semantic edit must NOT warm the stale entry"
    );
    assert!(
        recomputed
            .facts
            .decl_slot_seed(
                TopLevelOwnerId::ordinary_file(),
                "g",
                SemanticSymbolSpace::Value,
            )
            .is_some(),
        "the recomputed artifact must reflect the edited skeleton"
    );

    // The ReadSetSignature rail itself moved: the recorded parse facts
    // covering the artifact's inputs differ between the stale and the
    // fresh entry, so the stale entry's signature can never validate
    // against the live view.
    let fact_hash = |entry: &BinderIdentityFactsEntry, pick: &dyn Fn(&FactKey) -> bool| {
        entry
            .read_set_signature
            .facts
            .iter()
            .find_map(|fact| match fact {
                FactVersionRef::Parse(parse_fact) if pick(&parse_fact.key) => {
                    Some(parse_fact.expected_hash)
                }
                _ => None,
            })
            .unwrap_or_else(|| {
                panic!(
                    "entry must record the {:?} parse fact",
                    pick(&FactKey::SyntacticExportSet)
                )
            })
    };
    let export_set = |key: &FactKey| matches!(key, FactKey::SyntacticExportSet);
    let stale_export = fact_hash(&first, &export_set);
    let fresh_export = fact_hash(&recomputed, &export_set);
    assert_ne!(
        stale_export, fresh_export,
        "a semantic edit must move the recorded SyntacticExportSet rail \
         (the stale entry's ReadSetSignature invalidates)"
    );
    // The stale entry also recorded the per-declaration MemberShape
    // rail for `Foo`, proving the signature covers the artifact's
    // declaration inputs (not just the export set).
    let foo_shape = |key: &FactKey| matches!(key, FactKey::MemberShape { exporter, .. } if exporter.as_ref() == "Foo");
    let _ = fact_hash(&first, &foo_shape);
}

// ---------------------------------------------------------------------------
// Provenance served from the artifact in authored order
// ---------------------------------------------------------------------------

/// Overload-group membership and module/global augmentation
/// contribution order are served FROM THE ARTIFACT in authored order —
/// and the projection is computed in exactly one place (grep evidence:
/// no consumer re-walks raw `IndexedReady` for it).
#[test]
fn binder_provenance_served_from_artifact_in_authored_order() {
    const CANONICAL: &str = "/w/prov.ts";
    const SOURCE: &str = "\
function f(a: string): void;
function f(a: number): void;
function f(a: any) {}
declare module \"a-mod\" { interface First {} }
declare module \"a-mod\" { interface Second {} }
declare global { interface G1 {} }
";
    let host = make_host_with_file(CANONICAL, SOURCE);
    let facts = produce(&host, CANONICAL).facts.clone();

    // Overload group: membership + authored order (statement indices
    // 0, 1, 2 — the three `f` contributors in source order).
    let group = facts
        .overload_group(TopLevelOwnerId::ordinary_file(), "f")
        .expect("`f` is an overload group");
    assert_eq!(
        group.member_order.as_ref(),
        &[0u32, 1, 2],
        "overload-group member order must be the authored source order"
    );
    assert_eq!(group.seed.symbol_space, SemanticSymbolSpace::Value);

    // Declaration source order for the same slot matches.
    let order = facts
        .declaration_order
        .iter()
        .find(|record| {
            record.seed.merged_symbol_name.as_ref() == "f"
                && record.seed.symbol_space == SemanticSymbolSpace::Value
        })
        .expect("f must carry a declaration-order record");
    assert_eq!(order.contributor_order.as_ref(), &[0u32, 1, 2]);

    // Augmentation contribution order per scope, in authored order.
    // ORDER is the served fact (raw positions are compute-local sort
    // keys, never published — see the duplicate-contributor and
    // intra-block tests for the discriminating cases).
    let a_mod = AugmentationScopeKind::Module("a-mod".to_string());
    let module_contribs: Vec<_> = facts.augmentation_contributions_in_order(&a_mod).collect();
    assert_eq!(
        module_contribs
            .iter()
            .map(|c| c.name.as_ref())
            .collect::<Vec<_>>(),
        ["First", "Second"],
        "the module-augmentation contribution order must be the authored order"
    );
    assert_eq!(
        module_contribs
            .iter()
            .map(|c| c.contribution_order)
            .collect::<Vec<_>>(),
        [0u32, 1],
        "contribution_order must rank the authored sequence"
    );
    let global_contribs: Vec<_> = facts
        .augmentation_contributions_in_order(&AugmentationScopeKind::Global)
        .collect();
    assert_eq!(
        global_contribs
            .iter()
            .map(|c| c.name.as_ref())
            .collect::<Vec<_>>(),
        ["G1"],
    );

    // Grep evidence — the provenance projection is computed ONLY by the
    // artifact module; no consumer re-walks raw `IndexedReady` header
    // inventories for overload-group / augmentation-order data.
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let src_dir = manifest_dir.join("src");
    let mut offenders: Vec<String> = Vec::new();
    for path in rust_files_under(&src_dir) {
        let name = path
            .strip_prefix(&src_dir)
            .unwrap()
            .to_string_lossy()
            .to_string();
        if name == "binder_identity_facts.rs" {
            continue;
        }
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()));
        for needle in [
            "OverloadGroupRecord",
            "AugmentationContributionRecord",
            "augmentation_contributions_in_order",
        ] {
            if text.contains(needle) {
                offenders.push(format!("{name} references {needle}"));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "overload-group / augmentation-order provenance must be served ONLY from the \
         BinderIdentityFacts artifact module — no consumer may re-walk raw IndexedReady \
         for it. Offenders: {offenders:?}"
    );
}

fn rust_files_under(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        for entry in std::fs::read_dir(&current)
            .unwrap_or_else(|err| panic!("failed to list {}: {err}", current.display()))
        {
            let path = entry.expect("read_dir entry").path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                out.push(path);
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// binder_scope_id enters context-sensitive query identity
// ---------------------------------------------------------------------------

/// A query whose result depends on the lexical scope it is resolved
/// from (`ResolveDecl`) carries `binder_scope_id` in its
/// `SemanticQueryKey` identity as a content-free resolution-context
/// discriminator: two keys identical except for the binder scope id are
/// DISTINCT, and the id is derived from the scope's structural header
/// (never a content/version hash).
#[test]
fn binder_scope_id_enters_context_sensitive_query_identity() {
    let owner = TopLevelOwnerId::ordinary_file();
    let scope_alpha = ScopeId::file(Arc::from("/w/a.ts"), owner);
    let mut scope_beta = scope_alpha.clone();
    scope_beta.binder_scope_id = BinderScopeId::namespace_scope(owner, Arc::from("Beta"));

    let key = |scope: &ScopeId| {
        SemanticQueryKey::ResolveDecl(verter_session::semantic_query::ResolveDeclKey {
            scope: scope.clone(),
            name: Arc::from("Foo"),
        })
    };
    assert_ne!(
        key(&scope_alpha),
        key(&scope_beta),
        "two ResolveDecl keys differing only in binder_scope_id must be distinct"
    );
    assert_eq!(
        key(&scope_alpha),
        key(&scope_alpha.clone()),
        "identical binder_scope_id converges to the same query identity"
    );

    // The ScopeId::file constructor populates the file-scope id — the
    // same structural id the family-A artifact records for the file
    // top-level scope (the query-identity projection of family A).
    assert_eq!(
        scope_alpha.binder_scope_id,
        BinderScopeId::file_scope(owner),
        "ScopeId::file must carry the file top-level binder scope id"
    );
}

// ---------------------------------------------------------------------------
// Guard — the substrate is produced from the shallow inventory, never from a navigation index (production-shape evidence)
// ---------------------------------------------------------------------------

/// `BinderIdentityFacts` is produced FROM `IndexedReady` by the
/// family-A producer and consumed by the `U2` reducers BEFORE they
/// run; no `N0` navigation/location projection produces it (the
/// navigation layer is a pure PROJECTION over this substrate).
/// Structural evidence over the source tree.
#[test]
fn binder_identity_facts_are_pre_u2_and_not_n0_owned() {
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let producer_src = std::fs::read_to_string(manifest_dir.join("src/binder_identity_facts.rs"))
        .expect("the family-A artifact module must exist");
    // Produced FROM IndexedReady: the demand producer serves the
    // canonical post-parse artifact through the resolver-tier accessor.
    assert!(
        producer_src.contains("ensure_indexed_ready_serve"),
        "the BinderIdentityFacts producer must be demand-produced FROM IndexedReady"
    );
    assert!(
        producer_src.contains("project_binder_identity_facts"),
        "the artifact must be a typed projection over the shallow inventory"
    );

    // Not N0-owned: nothing named `n0` / `nav_location` may produce the
    // artifact; the only files referencing the substrate are the
    // artifact module itself, the store home (`project_type_store`),
    // the crate root, and the test-support shim.
    let src_dir = manifest_dir.join("src");
    let allowed = [
        "binder_identity_facts.rs",
        "project_type_store.rs",
        "lib.rs",
        "for_tests.rs",
        // The query-identity type surface — hosts `BinderScopeId` /
        // `DeclarationSlotSeed` (the query-identity projection of the
        // substrate), not a producer.
        "semantic_query.rs",
    ];
    let mut offenders: Vec<String> = Vec::new();
    for path in rust_files_under(&src_dir) {
        let name = path
            .strip_prefix(&src_dir)
            .unwrap()
            .to_string_lossy()
            .to_string();
        if allowed.iter().any(|a| *a == name) {
            continue;
        }
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()));
        if text.contains("BinderIdentityFacts") {
            offenders.push(name.clone());
        }
        let lowered = name.to_ascii_lowercase();
        assert!(
            !lowered.contains("n0") && !lowered.contains("nav_location"),
            "no N0 navigation/location producer module may exist for the \
             binder-identity substrate (found {name})"
        );
    }
    assert!(
        offenders.is_empty(),
        "BinderIdentityFacts references must stay inside the artifact module / store / \
         crate root / test shim (no N0 or reducer producer yet). Offenders: {offenders:?}"
    );
}

// ---------------------------------------------------------------------------
// Negative lookup stays ReturnOnly
// ---------------------------------------------------------------------------

/// A negative (name-not-found) binder answer is NOT backed by a
/// recorded corpus completeness fact in this block, so it routes
/// `ReturnOnly` — never a warm-cached miss.
#[test]
fn negative_name_lookup_requires_recorded_completeness_or_returnonly() {
    const CANONICAL: &str = "/w/neg.ts";
    let host = make_host_with_file(CANONICAL, "export interface Foo { a: string }\n");
    let facts = produce(&host, CANONICAL).facts.clone();
    let store = host.project_type_store().binder_identity_facts_store();
    let len_before = store.len();

    // A name absent from the file's own inventory is a per-file
    // negative ONLY: no ambient / global / lib contributor from another
    // file is visible to this artifact, so the negative is not
    // corpus-backed.
    assert!(
        facts
            .decl_slot_seed(
                TopLevelOwnerId::ordinary_file(),
                "DefinitelyMissing",
                SemanticSymbolSpace::Type,
            )
            .is_none(),
        "a missing name yields no seed"
    );
    assert!(
        matches!(negative_lookup_admission(), Admission::ReturnOnly),
        "a negative binder lookup without corpus completeness must route ReturnOnly"
    );
    assert_eq!(
        store.len(),
        len_before,
        "a negative lookup must NOT admit a warm-cached miss entry"
    );
}

// ---------------------------------------------------------------------------
// Order/set augmentation pins, contribution-order rails,
// ---------------------------------------------------------------------------

/// `true` when the entry's recorded read-set contains a parse fact
/// matching `pick`.
fn has_parse_fact(entry: &BinderIdentityFactsEntry, pick: &dyn Fn(&FactKey) -> bool) -> bool {
    entry
        .read_set_signature
        .facts
        .iter()
        .any(|fact| matches!(fact, FactVersionRef::Parse(parse_fact) if pick(&parse_fact.key)))
}

/// Adding a `declare module "m" { … }` contribution with an
/// UNCHANGED parse-stable skeleton must NOT warm-hit the stale entry:
/// the per-target contribution set fact moves, the warm read misses,
/// and the recomputed artifact includes the new contributor.
#[test]
fn module_aug_add_with_same_skeleton_warm_misses_and_recomputes() {
    const CANONICAL: &str = "/w/augadd.ts";
    let host = make_host_with_file(
        CANONICAL,
        "declare module \"a-mod\" { interface First {} }\n",
    );
    let first = produce(&host, CANONICAL);
    assert!(
        has_parse_fact(&first, &|key| {
            matches!(key, FactKey::AugmentationContributionSet { .. })
        }),
        "the signature must pin the per-target AugmentationContributionSet fact"
    );

    // Add a second contribution in a NEW block. The file-surface
    // skeleton (and therefore `parse_stable_hash`) is UNCHANGED — only
    // the augmentation inventory grew.
    upsert(
        &host,
        CANONICAL,
        "declare module \"a-mod\" { interface First {} }\ndeclare module \"a-mod\" { interface Second {} }\n",
    );
    let _ = host.analyze_with_audit(CANONICAL);
    let recomputed = produce(&host, CANONICAL);
    assert!(
        !Arc::ptr_eq(&first, &recomputed),
        "an augmentation-set edit must NOT warm-hit the stale entry \
         (the contribution-set fact must move)"
    );
    let a_mod = AugmentationScopeKind::Module("a-mod".to_string());
    let names: Vec<_> = recomputed
        .facts
        .augmentation_contributions_in_order(&a_mod)
        .map(|c| c.name.as_ref())
        .collect();
    assert_eq!(
        names,
        ["First", "Second"],
        "the recomputed artifact must include the new contributor in authored order"
    );
    assert_eq!(
        host.project_type_store()
            .binder_identity_facts_store()
            .len(),
        1,
        "the stale entry is replaced under the same key, not accumulated"
    );
}

/// A `declare global { … }` contribution is pinned by the
/// per-record `ModuleAugmentation` fact (the `$global` sentinel
/// specifier), so a declare-global ADD or EDIT warm-misses just like a
/// module-augmentation change.
#[test]
fn declare_global_add_or_edit_warm_misses() {
    const CANONICAL: &str = "/w/glob.ts";
    let host = make_host_with_file(CANONICAL, "declare global { interface G1 {} }\n");
    let first = produce(&host, CANONICAL);
    assert!(
        has_parse_fact(&first, &|key| {
            matches!(
                key,
                FactKey::ModuleAugmentation { specifier, augmented_name, .. }
                    if specifier.as_ref() == "$global" && augmented_name.as_ref() == "G1"
            )
        }),
        "the signature must pin the per-record `ModuleAugmentation` fact for the \
         `declare global` contribution (the `$global` sentinel specifier)"
    );

    // ADD a second global contribution inside the same block.
    upsert(
        &host,
        CANONICAL,
        "declare global { interface G1 {} interface G2 {} }\n",
    );
    let _ = host.analyze_with_audit(CANONICAL);
    let added = produce(&host, CANONICAL);
    assert!(
        !Arc::ptr_eq(&first, &added),
        "a declare-global ADD must warm-miss"
    );

    // EDIT the first global contribution's member set (G2 holds its
    // shape; only G1's header fingerprint moves).
    upsert(
        &host,
        CANONICAL,
        "declare global { interface G1 { member: string } interface G2 {} }\n",
    );
    let _ = host.analyze_with_audit(CANONICAL);
    let edited = produce(&host, CANONICAL);
    assert!(
        !Arc::ptr_eq(&added, &edited),
        "a declare-global member EDIT must warm-miss (the per-record rail moves)"
    );
}

/// Order-sensitive contributor-order rails: swapping an overload
/// group's signatures or two same-file exported declarations
/// warm-misses (the `DeclContributionOrder` fact folds the AUTHORED
/// contributor sequence), while a cosmetic comment BETWEEN overloads
/// keeps the warm hit (negative control — no body hash, no whole-hash,
/// no source-position folding).
#[test]
fn contributor_order_swap_warm_misses_cosmetic_between_overloads_stays_warm() {
    const CANONICAL: &str = "/w/order.ts";
    const V1: &str = "export function f(a: string): void;\nexport function f(a: number): void;\nexport interface A { a: string }\nexport interface B { b: number }\n";
    let host = make_host_with_file(CANONICAL, V1);
    let first = produce(&host, CANONICAL);
    assert!(
        has_parse_fact(&first, &|key| {
            matches!(
                key,
                FactKey::DeclContributionOrder { name, space, .. }
                    if name.as_ref() == "f" && matches!(space, verter_semantic::facts::SymbolSpace::Value)
            )
        }),
        "the signature must pin the DeclContributionOrder fact for the overload group"
    );

    // Negative control: a cosmetic comment BETWEEN the overloads —
    // statement indices and declaration slices are unchanged, so every
    // pinned fact (including the order rail) holds and the entry warms.
    upsert(
        &host,
        CANONICAL,
        "export function f(a: string): void;\n// cosmetic comment between overloads\nexport function f(a: number): void;\nexport interface A { a: string }\nexport interface B { b: number }\n",
    );
    let _ = host.analyze_with_audit(CANONICAL);
    let warm = produce(&host, CANONICAL);
    assert!(
        Arc::ptr_eq(&first, &warm),
        "a cosmetic comment BETWEEN overloads must keep the warm hit \
         (the order rail folds authored declaration shapes, never positions)"
    );

    // Overload swap: `f(number)` now precedes `f(string)` — the
    // authored contributor sequence changed.
    upsert(
        &host,
        CANONICAL,
        "export function f(a: number): void;\nexport function f(a: string): void;\nexport interface A { a: string }\nexport interface B { b: number }\n",
    );
    let _ = host.analyze_with_audit(CANONICAL);
    let swapped = produce(&host, CANONICAL);
    assert!(
        !Arc::ptr_eq(&warm, &swapped),
        "an overload-group swap must warm-miss (the order rail moves)"
    );

    // Exported-interface swap: each slot's contributor index moves
    // (A: 0→1… well, both keep their names/members — only the AUTHORED
    // order changed). The recomputed artifact reports the NEW order.
    upsert(
        &host,
        CANONICAL,
        "export function f(a: number): void;\nexport function f(a: string): void;\nexport interface B { b: number }\nexport interface A { a: string }\n",
    );
    let _ = host.analyze_with_audit(CANONICAL);
    let iface_swapped = produce(&host, CANONICAL);
    assert!(
        !Arc::ptr_eq(&swapped, &iface_swapped),
        "an exported-interface swap must warm-miss (each slot's contributor index moves)"
    );
    let order_of = |facts: &BinderIdentityFacts, name: &str| -> u32 {
        facts
            .declaration_order
            .iter()
            .find(|r| {
                r.seed.merged_symbol_name.as_ref() == name
                    && r.seed.symbol_space == SemanticSymbolSpace::Type
            })
            .unwrap_or_else(|| panic!("{name} must carry a declaration-order record"))
            .contributor_order[0]
    };
    assert_eq!(order_of(&iface_swapped.facts, "B"), 2);
    assert_eq!(order_of(&iface_swapped.facts, "A"), 3);
}

/// A file with ONLY augmentation blocks still owns a file
/// top-level scope, and the augmentation scope's parent link points at
/// it (no dangling scope tree).
#[test]
fn pure_augmentation_file_has_file_scope_for_aug_parent() {
    const CANONICAL: &str = "/w/pureaug.ts";
    let host = make_host_with_file(CANONICAL, "declare module \"a-mod\" { interface A {} }\n");
    let facts = produce(&host, CANONICAL).facts.clone();

    let owner = TopLevelOwnerId::ordinary_file();
    let file_scope = facts
        .file_scope_id(owner)
        .expect("an augmentation-only file must still own a file top-level scope");
    let aug_scope = facts
        .scopes
        .iter()
        .find(|record| {
            matches!(
                &record.kind,
                BinderScopeKind::AugmentationModule { specifier } if specifier.as_ref() == "a-mod"
            )
        })
        .expect("the augmentation scope must be recorded");
    assert_eq!(
        aug_scope.parent,
        Some(file_scope),
        "the augmentation scope's parent must be the file top-level scope (no dangling tree)"
    );
}

/// A NEW augmentation target must invalidate even when the parse-stable
/// skeleton is unchanged: appending a first `declare module "m" {…}` to
/// a file that had NO augmentation blocks at all must warm-miss (the
/// whole-file augmentation-target set fact moves), and the recomputed
/// artifact must include the new target's scope + contribution.
#[test]
fn new_augmentation_target_warm_misses() {
    const CANONICAL: &str = "/w/newtarget.ts";
    let host = make_host_with_file(CANONICAL, "export interface Keep {}\n");
    let first = produce(&host, CANONICAL);
    assert!(
        has_parse_fact(&first, &|key| matches!(key, FactKey::AugmentationTargetSet)),
        "the signature must pin the whole-file AugmentationTargetSet fact"
    );

    upsert(
        &host,
        CANONICAL,
        "export interface Keep {}\ndeclare module \"m\" { interface Added {} }\n",
    );
    let _ = host.analyze_with_audit(CANONICAL);
    let recomputed = produce(&host, CANONICAL);
    assert!(
        !Arc::ptr_eq(&first, &recomputed),
        "appending a NEW augmentation target must warm-miss even with an \
         unchanged parse-stable skeleton"
    );
    let m = AugmentationScopeKind::Module("m".to_string());
    let names: Vec<_> = recomputed
        .facts
        .augmentation_contributions_in_order(&m)
        .map(|c| c.name.as_ref())
        .collect();
    assert_eq!(
        names,
        ["Added"],
        "the recomputed artifact must include the new target's contribution"
    );
    assert!(
        recomputed.facts.scopes.iter().any(|record| matches!(
            &record.kind,
            BinderScopeKind::AugmentationModule { specifier } if specifier.as_ref() == "m"
        )),
        "the recomputed artifact must include the \"m\" augmentation scope"
    );
}

/// Two declarations inside ONE `declare module "m" {…}` block share the
/// outer statement index — their contribution order must still be the
/// AUTHORED order (each declaration's own source position), never an
/// alphabetical tie-break.
#[test]
fn intra_block_contributions_keep_authored_order() {
    const CANONICAL: &str = "/w/intrablock.ts";
    let host = make_host_with_file(
        CANONICAL,
        "declare module \"m\" { interface Z {} interface A {} }\n",
    );
    let facts = produce(&host, CANONICAL).facts.clone();
    let m = AugmentationScopeKind::Module("m".to_string());
    let ordered: Vec<_> = facts
        .augmentation_contributions_in_order(&m)
        .map(|c| (c.name.as_ref(), c.contribution_order))
        .collect();
    assert_eq!(
        ordered,
        [("Z", 0u32), ("A", 1u32)],
        "intra-block contributions must keep the AUTHORED order (Z then A), \
         never an alphabetical tie-break"
    );

    // Swapping the two declarations inside the block moves the order
    // fact too (the recomputed payload flips).
    let first = produce(&host, CANONICAL);
    upsert(
        &host,
        CANONICAL,
        "declare module \"m\" { interface A {} interface Z {} }\n",
    );
    let _ = host.analyze_with_audit(CANONICAL);
    let swapped = produce(&host, CANONICAL);
    assert!(
        !Arc::ptr_eq(&first, &swapped),
        "an intra-block swap must warm-miss (the augmentation order fact moves)"
    );
    let swapped_order: Vec<_> = swapped
        .facts
        .augmentation_contributions_in_order(&m)
        .map(|c| c.name.as_ref())
        .collect();
    assert_eq!(swapped_order, ["A", "Z"]);
}

/// A `namespace Empty {}` block with zero members is still a named
/// lexical scope: the artifact records its namespace scope AND its
/// namespace-space declaration-slot seed. A file whose ONLY content is
/// that empty block still owns a file top-level scope, and the
/// namespace scope's parent link points at it (no dangling scope tree —
/// same contract as pure-augmentation / empty `declare module` files).
#[test]
fn empty_namespace_has_seed_and_scope() {
    const CANONICAL: &str = "/w/emptyns.ts";
    let host = make_host_with_file(CANONICAL, "namespace Empty {}\n");
    let facts = produce(&host, CANONICAL).facts.clone();

    let owner = TopLevelOwnerId::ordinary_file();
    assert!(
        facts
            .decl_slot_seed(owner, "Empty", SemanticSymbolSpace::Namespace)
            .is_some(),
        "an empty namespace block must record its namespace-space seed"
    );
    let ns_scope = facts
        .scopes
        .iter()
        .find(|record| {
            matches!(
                &record.kind,
                BinderScopeKind::Namespace { qualified_name }
                    if qualified_name.as_ref() == "Empty"
            )
        })
        .expect("an empty namespace block must record its namespace scope");
    let file_scope = facts
        .file_scope_id(owner)
        .expect("a namespace-only file must still own a file top-level scope");
    assert_eq!(
        ns_scope.parent,
        Some(file_scope),
        "the namespace scope's parent must be the file top-level scope (no dangling tree)"
    );
    // Parent existence is structural, not just id equality: the parent
    // id MUST appear as a record in the artifact's scopes list.
    assert!(
        facts.scopes.iter().any(|record| record.id == file_scope),
        "the file-scope parent id must exist in the artifact's scopes"
    );
}

/// An empty `declare module "m" {}` block still introduces the
/// augmentation scope (its entry) with the file top-level scope as
/// parent.
#[test]
fn empty_declare_module_has_scope_and_file_parent() {
    const CANONICAL: &str = "/w/emptymod.ts";
    let host = make_host_with_file(CANONICAL, "declare module \"m\" {}\n");
    let facts = produce(&host, CANONICAL).facts.clone();

    let owner = TopLevelOwnerId::ordinary_file();
    let file_scope = facts
        .file_scope_id(owner)
        .expect("an empty declare-module file must own a file top-level scope");
    let aug_scope = facts
        .scopes
        .iter()
        .find(|record| {
            matches!(
                &record.kind,
                BinderScopeKind::AugmentationModule { specifier } if specifier.as_ref() == "m"
            )
        })
        .expect("an empty `declare module \"m\" {}` must still record the augmentation scope");
    assert_eq!(aug_scope.parent, Some(file_scope));
}

/// Cosmetic edits INSIDE a declaration (an inline comment in an
/// interface body, a whitespace reformat of a function signature) leave
/// the payload AND the order rail unchanged — the entry stays warm.
/// Real reorders still move it (covered by the swap tests above).
#[test]
fn cosmetic_edit_inside_declaration_stays_warm() {
    const CANONICAL: &str = "/w/insdecl.ts";
    const V1: &str = "export interface A { a: string }\nexport function f(x: number): void;\n";
    let host = make_host_with_file(CANONICAL, V1);
    let first = produce(&host, CANONICAL);

    // An inline comment INSIDE the interface body + a whitespace
    // reformat INSIDE the function signature: neither touches the
    // declaration skeleton, so the order rail must hold.
    upsert(
        &host,
        CANONICAL,
        "export interface A { /* inline comment */ a: string }\nexport function f( x:number ): void;\n",
    );
    let _ = host.analyze_with_audit(CANONICAL);
    let warm = produce(&host, CANONICAL);
    assert!(
        Arc::ptr_eq(&first, &warm),
        "a cosmetic edit INSIDE a declaration must stay warm \
         (the order rail hashes the comment-stripped, whitespace-normalised form)"
    );
    assert_eq!(
        first.facts, warm.facts,
        "the payload is identical under an intra-declaration cosmetic edit"
    );
}

/// An EMPTY augmentation target pins its (bare-target) contribution
/// set/order facts from the block inventory, so an empty →
/// first-contribution edit moves a pinned hash and warm-misses — while
/// an empty → empty edit stays warm (negative control).
#[test]
fn empty_augmentation_target_first_contribution_warm_misses() {
    const CANONICAL: &str = "/w/emptytarget.ts";
    let host = make_host_with_file(CANONICAL, "declare module \"m\" {}\n");
    let first = produce(&host, CANONICAL);
    assert!(
        has_parse_fact(&first, &|key| {
            matches!(
                key,
                FactKey::AugmentationContributionSet {
                    scope_kind_tag: verter_semantic::facts::AugmentationScopeKindTag::Module,
                    specifier,
                    ..
                } if specifier.as_ref() == "m"
            )
        }),
        "an EMPTY augmentation target must pin its bare-target AugmentationContributionSet fact"
    );

    // Negative control: a cosmetic edit on the still-empty block keeps
    // the warm hit.
    upsert(
        &host,
        CANONICAL,
        "// cosmetic comment\ndeclare module \"m\" {}\n",
    );
    let _ = host.analyze_with_audit(CANONICAL);
    let warm = produce(&host, CANONICAL);
    assert!(
        Arc::ptr_eq(&first, &warm),
        "empty → empty (cosmetic) must stay warm"
    );

    // Empty → first contribution: the contribution set/order facts move.
    upsert(
        &host,
        CANONICAL,
        "declare module \"m\" { interface A {} }\n",
    );
    let _ = host.analyze_with_audit(CANONICAL);
    let added = produce(&host, CANONICAL);
    assert!(
        !Arc::ptr_eq(&warm, &added),
        "empty → first contribution must warm-miss (the bare-target hash moves)"
    );
    let m = AugmentationScopeKind::Module("m".to_string());
    let names: Vec<String> = added
        .facts
        .augmentation_contributions_in_order(&m)
        .map(|c| c.name.to_string())
        .collect();
    assert_eq!(
        names,
        ["A"],
        "the recomputed artifact must include the first contribution"
    );

    // Same for `declare global {}` → first inner declaration.
    const GLOBAL_EMPTY: &str = "/w/emptyglobal2.ts";
    let host2 = make_host_with_file(GLOBAL_EMPTY, "declare global {}\n");
    let g_first = produce(&host2, GLOBAL_EMPTY);
    assert!(
        has_parse_fact(&g_first, &|key| {
            matches!(
                key,
                FactKey::AugmentationContributionSet {
                    scope_kind_tag: verter_semantic::facts::AugmentationScopeKindTag::Global,
                    ..
                }
            )
        }),
        "an EMPTY `declare global {{}}` must pin its bare-target contribution set fact"
    );
    upsert(&host2, GLOBAL_EMPTY, "declare global { interface G {} }\n");
    let _ = host2.analyze_with_audit(GLOBAL_EMPTY);
    let g_added = produce(&host2, GLOBAL_EMPTY);
    assert!(
        !Arc::ptr_eq(&g_first, &g_added),
        "empty global → first inner declaration must warm-miss"
    );
    let global_names: Vec<String> = g_added
        .facts
        .augmentation_contributions_in_order(&AugmentationScopeKind::Global)
        .map(|c| c.name.to_string())
        .collect();
    assert_eq!(global_names, ["G"]);
}

/// `declare global {…}` and `declare module "$global" {…}` occupy
/// DISTINCT target identities (the scope-kind tag, never a string
/// match): swapping one empty form for the other warm-misses and the
/// served scope kind is correct; a non-empty module literally named
/// `$global` keeps working as a module.
#[test]
fn declare_global_vs_module_global_sentinel_are_distinct_targets() {
    const CANONICAL: &str = "/w/globalid.ts";
    let host = make_host_with_file(CANONICAL, "declare global {}\n");
    let first = produce(&host, CANONICAL);
    assert!(
        first
            .facts
            .scopes
            .iter()
            .any(|record| matches!(record.kind, BinderScopeKind::AugmentationGlobal)),
        "the empty declare-global file must serve the AugmentationGlobal scope"
    );

    // Swap the empty forms: same specifier string, DIFFERENT scope kind.
    upsert(&host, CANONICAL, "declare module \"$global\" {}\n");
    let _ = host.analyze_with_audit(CANONICAL);
    let swapped = produce(&host, CANONICAL);
    assert!(
        !Arc::ptr_eq(&first, &swapped),
        "swapping `declare global {{}}` ↔ `declare module \"$global\" {{}}` must warm-miss \
         (the scope-kind tag distinguishes the two target identities)"
    );
    let owner = TopLevelOwnerId::ordinary_file();
    let file_scope = swapped
        .facts
        .file_scope_id(owner)
        .expect("file scope must be present");
    let module_scope = swapped
        .facts
        .scopes
        .iter()
        .find(|record| {
            matches!(
                &record.kind,
                BinderScopeKind::AugmentationModule { specifier } if specifier.as_ref() == "$global"
            )
        })
        .expect("the served scope kind must be the MODULE form named `$global`");
    assert_eq!(module_scope.parent, Some(file_scope));
    assert!(
        !swapped
            .facts
            .scopes
            .iter()
            .any(|record| matches!(record.kind, BinderScopeKind::AugmentationGlobal)),
        "after the swap no AugmentationGlobal scope may remain"
    );

    // A non-empty module literally named `$global` works as a module.
    const GLOBAL_NAMED: &str = "/w/globalid2.ts";
    let host2 = make_host_with_file(
        GLOBAL_NAMED,
        "declare module \"$global\" { interface X {} }\n",
    );
    let facts2 = produce(&host2, GLOBAL_NAMED).facts.clone();
    let dollar_global = AugmentationScopeKind::Module("$global".to_string());
    let module_names: Vec<String> = facts2
        .augmentation_contributions_in_order(&dollar_global)
        .map(|c| c.name.to_string())
        .collect();
    assert_eq!(
        module_names,
        ["X"],
        "a non-empty `declare module \"$global\"` must keep working as a module"
    );
    assert!(
        facts2
            .augmentation_contributions_in_order(&AugmentationScopeKind::Global)
            .next()
            .is_none(),
        "no contribution may leak into the Global scope identity"
    );
}

/// Duplicate contributions of one symbol inside one augmentation block
/// keep PER-POSITION records in authored order — and reordering them
/// moves the order rail (the per-symbol collapse would serve stale
/// order `A,B,A`).
#[test]
fn duplicate_contributors_keep_per_position_order() {
    const CANONICAL: &str = "/w/dup.ts";
    let host = make_host_with_file(
        CANONICAL,
        "declare module \"m\" { interface A {} interface B {} interface A {} }\n",
    );
    let first = produce(&host, CANONICAL);
    let m = AugmentationScopeKind::Module("m".to_string());
    let names: Vec<String> = first
        .facts
        .augmentation_contributions_in_order(&m)
        .map(|c| c.name.to_string())
        .collect();
    assert_eq!(
        names,
        ["A", "B", "A"],
        "duplicate symbols keep both entries at their authored positions"
    );
    let orders: Vec<u32> = first
        .facts
        .augmentation_contributions_in_order(&m)
        .map(|c| c.contribution_order)
        .collect();
    assert_eq!(orders, [0u32, 1, 2]);

    // Reorder the duplicates: `A,B,A` → `A,A,B`. The per-position order
    // rail must move (a per-symbol collapse would hash identically).
    upsert(
        &host,
        CANONICAL,
        "declare module \"m\" { interface A {} interface A {} interface B {} }\n",
    );
    let _ = host.analyze_with_audit(CANONICAL);
    let swapped = produce(&host, CANONICAL);
    assert!(
        !Arc::ptr_eq(&first, &swapped),
        "reordering duplicate contributors must warm-miss (the per-position order rail moves)"
    );
    let swapped_names: Vec<String> = swapped
        .facts
        .augmentation_contributions_in_order(&m)
        .map(|c| c.name.to_string())
        .collect();
    assert_eq!(swapped_names, ["A", "A", "B"]);
}

/// The served augmentation contribution record publishes ORDER only:
/// raw positions (statement indices, span starts) are compute-local
/// sort keys and never appear on the served payload — they move on
/// cosmetic inserts while the signature's order rail ignores them, so a
/// served position would drift warm-vs-cold. Structural pin on the
/// record's field set.
#[test]
fn augmentation_contribution_record_publishes_order_not_positions() {
    let src = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/binder_identity_facts.rs"),
    )
    .expect("read binder_identity_facts.rs");
    let start = src
        .find("pub struct AugmentationContributionRecord {")
        .expect("the record struct must exist");
    let end = src[start..]
        .find("\n}")
        .map(|i| start + i)
        .expect("the record struct must close");
    let body = &src[start..end];
    for forbidden in ["statement_index", "authored_position", "span", "whole_hash"] {
        assert!(
            !body.contains(forbidden),
            "the served AugmentationContributionRecord must NOT carry `{forbidden}` — \
             positions are compute-local sort keys, never published. Body:\n{body}"
        );
    }
    assert!(
        body.contains("pub contribution_order: u32"),
        "the served record publishes the authored ORDER (the fact)"
    );
}

// ---------------------------------------------------------------------------
// `$global` per-rail discrimination (typed on EVERY rail, asserted directly)
// ---------------------------------------------------------------------------

fn registry_for(source: &str) -> verter_session::file_artifact_store::FileFacts {
    let shallow = verter_session::resolver_core::shallow_file_state::ShallowFileState::service_backed_for_test_with_hash(
        "/rails.ts",
        source,
        [0u8; 16],
    );
    let indexed = verter_session::project_type_store::IndexedReady::new_for_test_with_state(
        [0u8; 16],
        shallow,
        Arc::from(source),
        Arc::from(source),
    );
    verter_session::fact_emission::emit_parse_facts(&indexed).facts
}

fn set_key(tag: verter_semantic::facts::AugmentationScopeKindTag, specifier: &str) -> FactKey {
    FactKey::AugmentationContributionSet {
        scope_kind_tag: tag,
        specifier: verter_session::file_artifact_store::InternedSpecifier::from(specifier),
        owner: TopLevelOwnerId::ordinary_file(),
    }
}

fn order_key(tag: verter_semantic::facts::AugmentationScopeKindTag, specifier: &str) -> FactKey {
    FactKey::AugmentationContributionOrder {
        scope_kind_tag: tag,
        specifier: verter_session::file_artifact_store::InternedSpecifier::from(specifier),
        owner: TopLevelOwnerId::ordinary_file(),
    }
}

/// Rail 1 — the whole-file `AugmentationTargetSet` hash: `declare
/// global {}` and `declare module "$global" {}` hash DIFFERENTLY (the
/// scope-kind tag is folded into the target-set hash content).
#[test]
fn global_vs_module_global_target_set_hash_differs() {
    let global = registry_for("declare global {}\n");
    let module = registry_for("declare module \"$global\" {}\n");
    let hash_of = |facts: &verter_session::file_artifact_store::FileFacts| {
        facts
            .lookup_or_compute(&FactKey::AugmentationTargetSet)
            .expect("AugmentationTargetSet must be emitted")
            .semantic_hash
    };
    assert_ne!(
        hash_of(&global),
        hash_of(&module),
        "the AugmentationTargetSet hash must distinguish `declare global` from \
         `declare module \"$global\"` (the scope-kind tag is folded in)"
    );
}

/// Rail 2 — the typed FactKeys: the two forms occupy DISTINCT
/// `AugmentationContributionSet` / `AugmentationContributionOrder` key
/// identities (tagged), so a lookup for the wrong tag misses.
#[test]
fn global_vs_module_global_fact_keys_are_typed_distinct() {
    use verter_semantic::facts::AugmentationScopeKindTag;
    let global = registry_for("declare global { interface X {} }\n");
    let module = registry_for("declare module \"$global\" { interface X {} }\n");

    let g_set = set_key(AugmentationScopeKindTag::Global, "$global");
    let m_set = set_key(AugmentationScopeKindTag::Module, "$global");
    assert_ne!(g_set, m_set, "the set keys are typed-distinct");
    let g_order = order_key(AugmentationScopeKindTag::Global, "$global");
    let m_order = order_key(AugmentationScopeKindTag::Module, "$global");
    assert_ne!(g_order, m_order, "the order keys are typed-distinct");

    // Each source emits exactly its own tag's keys; the wrong tag misses.
    assert!(global.lookup_or_compute(&g_set).is_some());
    assert!(global.lookup_or_compute(&m_set).is_none());
    assert!(module.lookup_or_compute(&m_set).is_some());
    assert!(module.lookup_or_compute(&g_set).is_none());
    assert!(global.lookup_or_compute(&g_order).is_some());
    assert!(global.lookup_or_compute(&m_order).is_none());
    assert!(module.lookup_or_compute(&m_order).is_some());
    assert!(module.lookup_or_compute(&g_order).is_none());
}

/// Rail 3 — the hash CONTENT of each rail: even with equal keys, the
/// bare-target set and order hashes of the two EMPTY forms differ (the
/// tag is folded into the hash bytes, not only into the key).
#[test]
fn global_vs_module_global_rail_hash_content_differs() {
    use verter_semantic::facts::AugmentationScopeKindTag;
    let global = registry_for("declare global {}\n");
    let module = registry_for("declare module \"$global\" {}\n");

    let g_set_hash = global
        .lookup_or_compute(&set_key(AugmentationScopeKindTag::Global, "$global"))
        .expect("global bare-target set fact")
        .semantic_hash;
    let m_set_hash = module
        .lookup_or_compute(&set_key(AugmentationScopeKindTag::Module, "$global"))
        .expect("module bare-target set fact")
        .semantic_hash;
    assert_ne!(
        g_set_hash, m_set_hash,
        "the bare-target SET hash content must distinguish the two forms"
    );
    let g_order_hash = global
        .lookup_or_compute(&order_key(AugmentationScopeKindTag::Global, "$global"))
        .expect("global bare-target order fact")
        .semantic_hash;
    let m_order_hash = module
        .lookup_or_compute(&order_key(AugmentationScopeKindTag::Module, "$global"))
        .expect("module bare-target order fact")
        .semantic_hash;
    assert_ne!(
        g_order_hash, m_order_hash,
        "the bare-target ORDER hash content must distinguish the two forms"
    );
}
