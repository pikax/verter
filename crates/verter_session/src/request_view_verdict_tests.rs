//! A request view settles a validation answer only when the answer is a
//! function of what the view captured, and reuses it, valid or stale, only
//! in the exact context it was given in: the completion state and the
//! strict self-roots the validated evidence reaches.

use std::sync::Arc;

use verter_session_query::facts::fact_cache::{FactVersionRef, ResultReceipt};
use verter_session_query::facts::store_view::StoreView;

use crate::resolver_core::{CanonicalCompletionOverlay, RequestStoreView};
use crate::resolver_store::HostStoreView;
use crate::types::FileLanguage;
use crate::{HostConfig, UpsertRequest, VerterHost};

fn base_view() -> HostStoreView {
    let host = VerterHost::new_standalone(HostConfig::default());
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/proj/button.ts".to_string(),
            source: Arc::from("export interface ButtonProps { label: string }\n"),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("upsert button.ts must succeed");
    host.resolver_store_view_read().into_owned_view()
}

fn whole(level: u8) -> FactVersionRef {
    FactVersionRef::FileWholeHash {
        canonical_id: format!("/r/{level}.ts").into(),
        hash: [level; 16],
    }
}

/// `/r/0.ts` as an older version recorded it; the overlay holds `[0; 16]`.
fn stale_fact() -> FactVersionRef {
    FactVersionRef::FileWholeHash {
        canonical_id: "/r/0.ts".into(),
        hash: [0xEE; 16],
    }
}

/// An overlay holding every `/r/{level}.ts` at its own version.
fn overlay_through(levels: u8) -> Arc<CanonicalCompletionOverlay> {
    let overlay = Arc::new(CanonicalCompletionOverlay::new());
    for level in 0..levels {
        overlay.insert_whole_hash_for_tests(&format!("/r/{level}.ts"), [level; 16]);
    }
    overlay
}

/// Level `n` consumed level `n - 1` and read `/r/{n}.ts`; level 0 read the
/// stale fact.
fn stale_chain(make: impl Fn(Vec<FactVersionRef>) -> ResultReceipt) -> Vec<ResultReceipt> {
    let mut levels = vec![make(vec![stale_fact()])];
    for level in 1..32u8 {
        let below = FactVersionRef::Receipt(levels.last().expect("a level below").clone());
        levels.push(make(vec![below, whole(level)]));
    }
    levels
}

fn reads(
    view: &RequestStoreView<'_>,
    signature: &[FactVersionRef],
    roots: &[&str],
) -> (Result<(), usize>, usize) {
    let before = view.leaf_validations_for_tests();
    let answer = view.validate_fact_signature(
        verter_session_query::facts::fact_set::FactsRef::from(signature),
        roots,
    );
    (answer, view.leaf_validations_for_tests() - before)
}

fn receipt_stamp(
    signature: &[FactVersionRef],
) -> &verter_session_query::facts::receipt::PositiveValidationStamp {
    let FactVersionRef::Receipt(receipt) = &signature[0] else {
        panic!("fixture must hold a receipt");
    };
    receipt.validation_stamp()
}

fn stamp_reads(signature: &[FactVersionRef]) -> usize {
    receipt_stamp(signature).reads_for_tests()
}

#[test]
fn an_untracked_own_file_rejects_a_retained_signature() {
    let base = base_view();
    let view = RequestStoreView::new(&base, Arc::new(CanonicalCompletionOverlay::new()));
    let signature =
        verter_session_query::facts::policy::FactPolicy::production().set_of(vec![whole(99)]);
    assert!(view.validates_retained_signature(&signature, &[]));
    assert!(!view.validates_retained_signature(&signature, &["/r/99.ts"]));
    assert!(view.validates_retained_signature(&signature, &["/unrelated.ts"]));
}

#[test]
fn a_fully_strict_positive_receipt_reuses_validation_across_requests() {
    const ID: &str = "/proj/button.ts";
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_stamp_source(&host, ID, "export interface ButtonProps { label: string }");
    let base = host.resolver_store_view_read().into_owned_view();
    let fact = FactVersionRef::FileWholeHash {
        canonical_id: ID.into(),
        hash: base.whole_hash_for_tests(ID).expect("tracked file"),
    };
    let child = ResultReceipt::new(vec![fact]);
    let parent = ResultReceipt::new(vec![FactVersionRef::Receipt(child.clone())]);
    for (receipt, expected) in [(parent.clone(), 1), (parent.clone(), 0), (child, 0)] {
        let view = RequestStoreView::new(&base, Arc::new(CanonicalCompletionOverlay::new()));
        assert_eq!(
            reads(&view, &[FactVersionRef::Receipt(receipt)], &[ID]),
            (Ok(()), expected),
            "an unchanged token reuses a strictly validated positive verdict"
        );
    }
    upsert_stamp_source(&host, "/proj/unrelated.ts", "export const value = 1;");
    let moved = host.resolver_store_view_read().into_owned_view();
    assert_ne!(base.validation_token(), moved.validation_token());
    for expected in [1, 0] {
        let view = RequestStoreView::new(&moved, Arc::new(CanonicalCompletionOverlay::new()));
        assert_eq!(
            reads(&view, &[FactVersionRef::Receipt(parent.clone())], &[ID]),
            (Ok(()), expected),
            "a surviving receipt walks once and stamps the moved token"
        );
    }
}

#[test]
fn a_lazily_validated_receipt_cannot_vouch_for_a_strict_root() {
    let base = base_view();
    let receipt = ResultReceipt::new(vec![whole(99)]);
    let signature = [FactVersionRef::Receipt(receipt)];
    let lazy = RequestStoreView::new(&base, Arc::new(CanonicalCompletionOverlay::new()));
    assert_eq!(reads(&lazy, &signature, &[]), (Ok(()), 1));
    let strict = RequestStoreView::new(&base, Arc::new(CanonicalCompletionOverlay::new()));
    assert_eq!(reads(&strict, &signature, &["/r/99.ts"]), (Err(0), 1));
}

#[test]
fn tracked_dependencies_can_supply_a_strict_receipt_stamp() {
    const ID: &str = "/proj/button.ts";
    let base = base_view();
    let tracked = FactVersionRef::FileWholeHash {
        canonical_id: ID.into(),
        hash: base.whole_hash_for_tests(ID).expect("tracked dependency"),
    };
    for (fact, reusable) in [(tracked, true), (whole(99), false)] {
        let signature = [FactVersionRef::Receipt(ResultReceipt::new(vec![fact]))];
        for expected in [1, usize::from(!reusable)] {
            let view = RequestStoreView::new(&base, Arc::new(CanonicalCompletionOverlay::new()));
            assert_eq!(reads(&view, &signature, &[]), (Ok(()), expected));
            assert_eq!(
                receipt_stamp(&signature).token_for_tests().is_some(),
                reusable
            );
        }
    }
}

#[test]
fn a_stale_receipt_is_never_reused_as_a_positive_verdict() {
    const ID: &str = "/proj/button.ts";
    let base = base_view();
    let signature = [FactVersionRef::Receipt(ResultReceipt::new(vec![
        FactVersionRef::FileWholeHash {
            canonical_id: ID.into(),
            hash: [0xEE; 16],
        },
    ]))];
    for _ in 0..2 {
        let view = RequestStoreView::new(&base, Arc::new(CanonicalCompletionOverlay::new()));
        assert_eq!(reads(&view, &signature, &[ID]), (Err(0), 1));
    }
}

#[test]
fn completion_shadowing_never_reads_or_writes_a_positive_stamp() {
    const ID: &str = "/proj/button.ts";
    let base = base_view();
    let signature = [FactVersionRef::Receipt(ResultReceipt::new(vec![
        FactVersionRef::FileWholeHash {
            canonical_id: ID.into(),
            hash: base.whole_hash_for_tests(ID).expect("tracked file"),
        },
    ]))];
    let view = RequestStoreView::new(&base, Arc::new(CanonicalCompletionOverlay::new()));
    assert_eq!(reads(&view, &signature, &[ID]), (Ok(()), 1));
    let lookups = stamp_reads(&signature);
    for _ in 0..2 {
        let overlay = Arc::new(CanonicalCompletionOverlay::new());
        overlay.insert_whole_hash_for_tests(ID, [0xEE; 16]);
        let shadow = RequestStoreView::new(&base, overlay);
        assert_eq!(reads(&shadow, &signature, &[ID]), (Err(0), 1));
        assert_eq!(stamp_reads(&signature), lookups);
    }
    let overlay = Arc::new(CanonicalCompletionOverlay::new());
    overlay.insert_whole_hash_for_tests("/unrelated.ts", [9; 16]);
    let unstamped = [FactVersionRef::Receipt(ResultReceipt::new(vec![
        FactVersionRef::ProjectGeneration {
            generation: base.validation_token().project_generation,
        },
    ]))];
    for _ in 0..2 {
        let shadow = RequestStoreView::new(&base, Arc::clone(&overlay));
        assert_eq!(reads(&shadow, &unstamped, &[]), (Ok(()), 1));
        assert_eq!(stamp_reads(&unstamped), 0);
        assert!(receipt_stamp(&unstamped).token_for_tests().is_none());
    }
}

#[test]
fn session_populations_never_read_or_write_a_positive_stamp() {
    use crate::session_view::OverlaidView;
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let mut overlays = rustc_hash::FxHashMap::default();
    overlays.insert("/session.ts".to_owned(), Arc::from("export const a = 1;"));
    let session = OverlaidView::new(Arc::clone(&host), overlays);
    let base = host.resolver_store_view_read().into_owned_view();
    let overlaid = base.clone().with_session_overlay(&host, &session);
    assert_ne!(overlaid.view_population(), base.view_population());
    for stamped_in_base in [false, true] {
        let signature = [FactVersionRef::Receipt(ResultReceipt::new(vec![
            FactVersionRef::ProjectGeneration {
                generation: base.validation_token().project_generation,
            },
        ]))];
        if stamped_in_base {
            let view = RequestStoreView::new(&base, Arc::new(CanonicalCompletionOverlay::new()));
            assert_eq!(reads(&view, &signature, &[]), (Ok(()), 1));
        }
        let lookups = stamp_reads(&signature);
        let recorded = receipt_stamp(&signature).token_for_tests();
        for _ in 0..2 {
            let view =
                RequestStoreView::new(&overlaid, Arc::new(CanonicalCompletionOverlay::new()));
            assert_eq!(reads(&view, &signature, &[]), (Ok(()), 1));
            assert_eq!(stamp_reads(&signature), lookups);
            assert_eq!(receipt_stamp(&signature).token_for_tests(), recorded);
        }
    }
}

#[test]
fn a_stamp_lookup_refuses_a_completion_that_moves_during_the_lookup() {
    const ID: &str = "/proj/button.ts";
    let base = base_view();
    let receipt = ResultReceipt::new(vec![FactVersionRef::FileWholeHash {
        canonical_id: ID.into(),
        hash: base.whole_hash_for_tests(ID).expect("tracked"),
    }]);
    let signature = [FactVersionRef::Receipt(receipt.clone())];
    let first = RequestStoreView::new(&base, Arc::new(CanonicalCompletionOverlay::new()));
    assert_eq!(reads(&first, &signature, &[ID]), (Ok(()), 1));
    let overlay = Arc::new(CanonicalCompletionOverlay::new());
    let mover = Arc::clone(&overlay);
    let moving = RequestStoreView::new(&base, overlay).with_validation_step_hook_for_tests(
        Arc::new(move |step| {
            if step == 0 {
                mover.insert_whole_hash_for_tests(ID, [0xEE; 16]);
            }
        }),
    );
    assert!(!moving.reuses_positive_validation_stamp(receipt.validation_stamp()));
    assert_eq!(moving.peek_whole_hash_for_tests(ID), Some([0xEE; 16]));
    assert_eq!(reads(&moving, &signature, &[ID]), (Err(0), 1));
}

fn upsert_stamp_source(host: &VerterHost, id: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(id.to_owned()),
            input_id: id.to_owned(),
            source: Arc::from(source),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("source admitted");
    host.ensure_indexed_ready(id).expect("source indexed");
}

/// Compare a moved-token read with a newly minted, unstamped copy of the
/// same evidence. Both must take the complete ordinary validation path,
/// including its first failure and leaf count.
fn a_domain_move_rechecks_receipt(
    host: &VerterHost,
    facts: Vec<FactVersionRef>,
    roots: &[&str],
    change: impl FnOnce(),
) {
    let before = host.resolver_store_view_read().into_owned_view();
    let receipt = ResultReceipt::new(facts.clone());
    let signature = [FactVersionRef::Receipt(receipt)];
    let first = RequestStoreView::new(&before, Arc::new(CanonicalCompletionOverlay::new()));
    let (answer, work) = reads(&first, &signature, roots);
    assert_eq!(answer, Ok(()));
    assert!(work > 0);
    let repeat = RequestStoreView::new(&before, Arc::new(CanonicalCompletionOverlay::new()));
    assert_eq!(reads(&repeat, &signature, roots), (Ok(()), 0));
    change();
    let after = host.resolver_store_view_read().into_owned_view();
    let control = [FactVersionRef::Receipt(ResultReceipt::new(facts))];
    let ordinary = RequestStoreView::new(&after, Arc::new(CanonicalCompletionOverlay::new()));
    let expected = reads(&ordinary, &control, roots);
    assert!(expected.0.is_err(), "the changed fact must be stale");
    assert!(expected.1 > 0, "the ordinary path must walk");
    assert_ne!(
        before.validation_token(),
        after.validation_token(),
        "the ordinary validation found stale evidence with {expected:?}"
    );
    for _ in 0..2 {
        let moved = RequestStoreView::new(&after, Arc::new(CanonicalCompletionOverlay::new()));
        assert_eq!(reads(&moved, &signature, roots), expected);
    }
}

#[test]
fn a_parse_domain_move_alone_rechecks_a_positive_receipt() {
    use verter_session_query::facts::fact_cache::ParseFactRef;
    use verter_session_query::facts::{FactKey, FactLane};
    const ID: &str = "/proj/parse.ts";
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_stamp_source(&host, ID, "export const a = 1;");
    let indexed = host.ensure_indexed_ready(ID).expect("indexed");
    let key = FactKey::SyntacticExportSet;
    let artifact_key = host
        .authoritative_current_artifact_key(ID)
        .expect("artifact key");
    let artifacts = host
        .project_type_store()
        .indexed()
        .get_artifacts(&artifact_key)
        .expect("artifact");
    assert!(Arc::ptr_eq(&indexed, &artifacts.indexed));
    let hash = artifacts
        .facts
        .lookup(&key)
        .expect("export set")
        .semantic_hash;
    let token = host.current_validation_token();
    a_domain_move_rechecks_receipt(
        &host,
        vec![FactVersionRef::Parse(ParseFactRef {
            canonical_id: ID.into(),
            key,
            lane: FactLane::Semantic,
            expected_hash: hash,
        })],
        &[],
        || {
            let mut next = artifacts.as_ref().clone();
            let mut facts = next.facts.as_ref().clone();
            let mut fact = facts
                .lookup(&FactKey::SyntacticExportSet)
                .expect("export set")
                .clone();
            fact.semantic_hash[0] ^= 1;
            facts.registry_mut().insert(fact);
            next.facts = Arc::new(facts);
            host.project_type_store()
                .indexed()
                .insert_artifacts(artifact_key, Arc::new(next));
            let mut moved = host.current_validation_token();
            assert_ne!(moved.artifact_generation, token.artifact_generation);
            moved.artifact_generation = token.artifact_generation;
            assert_eq!(moved, token, "only parse facts were republished");
        },
    );
}

#[test]
fn a_resolution_domain_move_alone_rechecks_a_positive_receipt() {
    use verter_session_query::resolution::{ResolutionContext, ResolvePhase, ResolveRequestKind};
    const ID: &str = "/proj/importer.ts";
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_stamp_source(
        &host,
        ID,
        "import { value } from './dep'; export { value };",
    );
    let context = ResolutionContext {
        phase: ResolvePhase::ProviderGraph,
        kind: ResolveRequestKind::EsmImport,
    };
    let outcome = host.ws().resolve_import_outcome(ID, "./dep", context);
    let verter_workspace::ResolutionPublication::Admitted(admitted) = outcome.into_publication()
    else {
        panic!("resolution witness must be admitted");
    };
    let facts = admitted
        .signature()
        .facts
        .iter()
        .cloned()
        .collect::<Vec<_>>();
    assert!(facts
        .iter()
        .any(|fact| matches!(fact, FactVersionRef::ResolveImports(_))));
    let token = host.current_validation_token();
    a_domain_move_rechecks_receipt(&host, facts, &[], || {
        let applied = host.ws().set_exact_resolutions(
            ID,
            vec![verter_workspace::ExactResolution {
                specifier: "./dep".to_owned(),
                phase: context.phase,
                kind: context.kind,
                resolved_canonical_id: Some("/proj/dep.ts".to_owned()),
                possible_canonical_ids: vec!["/proj/dep.ts".to_owned()],
            }],
        );
        assert!(applied.changed);
        let mut moved = host.current_validation_token();
        assert_ne!(
            moved.resolution_fact_generation,
            token.resolution_fact_generation
        );
        moved.resolution_fact_generation = token.resolution_fact_generation;
        assert_eq!(moved, token, "only resolution currency moved");
    });
}

#[test]
fn a_route_surface_domain_move_alone_rechecks_a_positive_receipt() {
    use crate::file_artifact_store::{
        AugmentationPopulation, AugmentationTargetKey, AugmentationTargetKind,
    };
    use verter_session_query::facts::fact_cache::RouteSurfaceFactRef;
    use verter_session_query::facts::{AugmentationTargetKindTag, FactKey, FactLane};
    use verter_session_query::resolution::AugmenterSet;
    let host = VerterHost::new_standalone(HostConfig::default());
    let env = host.host_view_env_hashes();
    let project_identity = host.host_view_project_identity();
    let key = AugmentationTargetKey {
        project_identity,
        resolve_env_hash: env.resolve_env_hash,
        lib_env_hash: env.lib_env_hash,
        population: AugmentationPopulation::Base,
        target: AugmentationTargetKind::GlobalAugmentation,
    };
    let publish = |hash| {
        host.project_type_store().indexed().populate_augmenter_set(
            key.clone(),
            Arc::new(AugmenterSet {
                entries: Default::default(),
                fingerprint: hash,
            }),
        )
    };
    publish([7; 16]);
    let token = host.current_validation_token();
    a_domain_move_rechecks_receipt(
        &host,
        vec![FactVersionRef::RouteSurface(RouteSurfaceFactRef {
            canonical_id: "/proj/ambient.ts".into(),
            key: FactKey::ModuleAugmentationIndexShape {
                target_kind_tag: AugmentationTargetKindTag::GlobalAugmentation,
                external_specifier: None,
                resolved_relative_canonical: None,
                wildcard_pattern: None,
            },
            lane: FactLane::Semantic,
            expected_hash: [7; 16],
        })],
        &[],
        || {
            publish([8; 16]);
            let mut moved = host.current_validation_token();
            assert_ne!(moved.artifact_generation, token.artifact_generation);
            moved.artifact_generation = token.artifact_generation;
            assert_eq!(moved, token, "only the route population changed");
        },
    );
}

#[test]
fn a_program_analysis_domain_move_alone_rechecks_a_positive_receipt() {
    use verter_session_query::facts::fact_cache::{
        ProgramAnalysisFactRef, ProgramAnalysisFunctionRef,
    };
    const ID: &str = "/proj/body.ts";
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_stamp_source(&host, ID, "export function value() { return 1; }");
    let indexed = host.ensure_indexed_ready(ID).expect("indexed");
    let function = ProgramAnalysisFunctionRef {
        canonical_id: ID.into(),
        owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
        merged_symbol_name: "value".into(),
        symbol_space: verter_session_query::facts::SymbolSpace::Value,
        function_part: verter_type_expr::facts::FunctionPartIdentity::DeclarationBody,
        overload_ordinal: 0,
    };
    let hash = indexed
        .shallow_state
        .decl_bodies()
        .function_program_index()
        .value
        .value_function(function.owner, "value", &function.function_part, 0)
        .expect("function indexed")
        .entry()
        .flow_body_stable_hash();
    let artifact_key = host
        .authoritative_current_artifact_key(ID)
        .expect("artifact key");
    let artifacts = host
        .project_type_store()
        .indexed()
        .get_artifacts(&artifact_key)
        .expect("artifact");
    let alternate = VerterHost::new_standalone(HostConfig::default());
    upsert_stamp_source(&alternate, ID, "export function value() { return 2; }");
    let alternate_indexed = alternate
        .ensure_indexed_ready(ID)
        .expect("alternate indexed");
    assert_ne!(
        alternate_indexed
            .shallow_state
            .decl_bodies()
            .function_program_index()
            .value
            .value_function(function.owner, "value", &function.function_part, 0)
            .expect("alternate function indexed")
            .entry()
            .flow_body_stable_hash(),
        hash
    );
    let token = host.current_validation_token();
    a_domain_move_rechecks_receipt(
        &host,
        vec![FactVersionRef::ProgramAnalysis(
            ProgramAnalysisFactRef::FlowBody {
                function,
                flow_body_stable_hash: hash,
            },
        )],
        &[],
        || {
            let mut next = artifacts.as_ref().clone();
            let mut program = next.indexed.as_ref().clone();
            program.shallow_state = Arc::clone(&alternate_indexed.shallow_state);
            next.indexed = Arc::new(program);
            assert!(Arc::ptr_eq(&next.facts, &artifacts.facts));
            assert_eq!(next.indexed.whole_hash, indexed.whole_hash);
            assert!(Arc::ptr_eq(&next.indexed.raw_source, &indexed.raw_source));
            host.project_type_store()
                .indexed()
                .insert_artifacts(artifact_key, Arc::new(next));
            let mut moved = host.current_validation_token();
            assert_ne!(moved.artifact_generation, token.artifact_generation);
            moved.artifact_generation = token.artifact_generation;
            assert_eq!(moved, token, "only program analysis was republished");
        },
    );
}

#[test]
fn a_source_environment_move_rechecks_a_positive_receipt_with_unchanged_content() {
    const ID: &str = "/proj/language.ts";
    const SOURCE: &str = "export const value = 1;";
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_stamp_source(&host, ID, SOURCE);
    let key = host
        .authoritative_current_artifact_key(ID)
        .expect("source identity");
    let whole_hash = key.content_hash;
    let source_env = crate::resolver_store::live_source_env_identity(&host, &key);
    a_domain_move_rechecks_receipt(
        &host,
        vec![FactVersionRef::FileSourceEnv {
            canonical_id: ID.into(),
            parse_env_hash: source_env.parse_env_hash,
            parse_key: source_env.parse_key,
            file_language_id: source_env.file_language_id,
        }],
        &[],
        || {
            let _ = host
                .upsert(UpsertRequest {
                    canonical_id: Some(ID.to_owned()),
                    input_id: ID.to_owned(),
                    source: Arc::from(SOURCE),
                    file_language: FileLanguage::script(verter_language::ScriptSourceType::Tsx),
                    aliases: Vec::new(),
                })
                .expect("new language admitted");
            host.ensure_indexed_ready(ID).expect("new language indexed");
            assert_eq!(
                host.authoritative_current_artifact_key(ID)
                    .expect("identity")
                    .content_hash,
                whole_hash
            );
        },
    );
}

#[test]
fn a_semantic_imports_domain_move_alone_rechecks_a_positive_receipt() {
    use crate::session_view::{HostView, SessionView};
    use verter_session_query::facts::fact_cache::ResolveImportsFactRef;
    use verter_session_query::facts::registry::{InternedName, InternedSpecifier};
    use verter_session_query::facts::{FactKey, FactLane};
    use verter_session_query::resolution::{ResolutionContext, ResolvePhase, ResolveRequestKind};
    const ID: &str = "/proj/importer.ts";
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    upsert_stamp_source(&host, "/proj/dep.ts", "export const value = 1;");
    upsert_stamp_source(&host, "/proj/other.ts", "export const value = 2;");
    upsert_stamp_source(
        &host,
        ID,
        "import { value } from './dep'; export const result = value;",
    );
    let routes = rustc_hash::FxHashMap::from_iter([(
        "./dep".to_owned(),
        crate::types::DependencyResolution {
            specifier: "./dep".to_owned(),
            resolved_canonical_id: Some("/proj/dep.ts".to_owned()),
            possible_canonical_ids: Vec::new(),
        },
    )]);
    assert!(host.admit_resolved_import_facts_for_owner(ID, &routes));
    let session = HostView::new(Arc::clone(&host));
    let payload = session
        .resolved_import_facts(ID)
        .expect("resolved import facts");
    let entry = payload
        .import_clauses
        .iter()
        .find(|entry| entry.binding.as_ref() == "value")
        .expect("binding");
    let fact = FactVersionRef::ResolveImports(ResolveImportsFactRef::Semantic {
        canonical_id: ID.into(),
        key: FactKey::ResolvedImportClause {
            specifier: InternedSpecifier::from(entry.specifier.as_ref()),
            binding: InternedName::from(entry.binding.as_ref()),
            space: entry.space,
            resolved_canonical: entry.resolved_canonical.as_ref().expect("resolved").clone(),
            resolved_source_name: InternedName::from(entry.resolved_source_name.as_ref()),
        },
        lane: FactLane::Semantic,
        expected_hash: entry.fact.semantic_hash,
    });
    let token = host.current_validation_token();
    a_domain_move_rechecks_receipt(&host, vec![fact], &[], || {
        let context = ResolutionContext {
            phase: ResolvePhase::CodegenBlocker,
            kind: ResolveRequestKind::TypeImport,
        };
        assert!(
            host.ws()
                .set_exact_resolutions(
                    ID,
                    vec![verter_workspace::ExactResolution {
                        specifier: "./dep".to_owned(),
                        phase: context.phase,
                        kind: context.kind,
                        resolved_canonical_id: Some("/proj/other.ts".to_owned()),
                        possible_canonical_ids: vec!["/proj/other.ts".to_owned()],
                    }]
                )
                .changed
        );
        let mut moved_routes = routes.clone();
        moved_routes
            .get_mut("./dep")
            .expect("import route")
            .resolved_canonical_id = Some("/proj/other.ts".to_owned());
        assert!(host.admit_resolved_import_facts_for_owner(ID, &moved_routes));
        let moved = session
            .resolved_import_facts(ID)
            .expect("retargeted import facts");
        assert_eq!(
            moved
                .import_clauses
                .iter()
                .find(|entry| entry.binding.as_ref() == "value")
                .expect("retargeted binding")
                .resolved_canonical
                .as_deref(),
            Some("/proj/other.ts")
        );
        let mut current = host.current_validation_token();
        assert_ne!(
            current.resolution_fact_generation,
            token.resolution_fact_generation
        );
        current.resolution_fact_generation = token.resolution_fact_generation;
        assert_eq!(current, token, "only import resolution currency moved");
    });
}

#[test]
fn token_movement_rechecks_whole_file_derived_aggregate_and_world_evidence() {
    use verter_session_query::facts::fact_cache::{
        AggregatePopulation, AggregateStamp, CompactionDomain, DerivedFactKind,
        DomainGenerationFact, ViewPopulation,
    };
    const ID: &str = "/proj/content.ts";
    for kind in 0..5 {
        let host = VerterHost::new_standalone(HostConfig::default());
        upsert_stamp_source(&host, ID, "export const value = 1;");
        let base = host.resolver_store_view_read().into_owned_view();
        let hash = base.whole_hash_for_tests(ID).expect("tracked");
        let fact = match kind {
            0 => FactVersionRef::FileWholeHash {
                canonical_id: ID.into(),
                hash,
            },
            1 => FactVersionRef::DerivedFactHash {
                canonical_id: ID.into(),
                kind: DerivedFactKind::DirectSource,
                hash,
            },
            2 => FactVersionRef::DomainGeneration(DomainGenerationFact {
                domain: CompactionDomain::Content,
                population: AggregatePopulation::View(ViewPopulation::Base),
                stamp: AggregateStamp::Generation(base.validation_token().content_generation),
            }),
            3 => FactVersionRef::StrictSelfRootWorld(
                base.strict_self_root_world_identity()
                    .expect("strict world"),
            ),
            4 => FactVersionRef::ProjectGeneration {
                generation: base.validation_token().project_generation,
            },
            _ => unreachable!(),
        };
        a_domain_move_rechecks_receipt(&host, vec![fact], &[ID], || {
            if kind == 4 {
                host.configure_projects(Vec::new());
            } else {
                upsert_stamp_source(&host, ID, "export const value = 2;");
            }
        });
    }
}

#[test]
fn small_strict_root_sets_do_not_build_an_index() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let base = host.resolver_store_view_read().into_owned_view();
    for roots in [
        vec![],
        vec!["/r/99.ts"],
        vec!["/other.ts", "/r/99.ts"],
        vec!["/a.ts", "/b.ts", "/c.ts", "/r/99.ts"],
    ] {
        let view = RequestStoreView::new(&base, Arc::new(CanonicalCompletionOverlay::new()));
        let signature =
            verter_session_query::facts::policy::FactPolicy::production().set_of(vec![whole(99)]);
        let before = crate::for_tests::root_index_builds_for_tests();
        assert_eq!(
            view.validates_retained_signature(&signature, &roots),
            roots.is_empty(),
            "an untracked own file rejects strictly"
        );
        assert_eq!(crate::for_tests::root_index_builds_for_tests() - before, 0);
    }

    let world = base
        .strict_self_root_world_identity()
        .expect("fixture world");
    let roots = [
        "/a.ts", "/b.ts", "/c.ts", "/d.ts", "/e.ts", "/f.ts", "/g.ts", "/h.ts",
    ];
    for fact in [
        FactVersionRef::StrictSelfRootWorld(world),
        FactVersionRef::Receipt(ResultReceipt::new(vec![
            FactVersionRef::StrictSelfRootWorld(world),
        ])),
    ] {
        let view = RequestStoreView::new(&base, Arc::new(CanonicalCompletionOverlay::new()));
        let signature =
            verter_session_query::facts::policy::FactPolicy::production().set_of(vec![fact]);
        let before = crate::for_tests::root_index_builds_for_tests();
        assert!(view.validates_retained_signature(&signature, &roots));
        assert_eq!(crate::for_tests::root_index_builds_for_tests() - before, 0);
    }

    for size in [1u8, 2, 4, 7, 8] {
        let view = RequestStoreView::new(&base, overlay_through(size));
        let signature = verter_session_query::facts::policy::FactPolicy::production()
            .set_of((0..size).map(whole).collect::<Vec<_>>());
        let roots: Vec<_> = (0..size).map(|level| format!("/r/{level}.ts")).collect();
        let roots: Vec<_> = roots.iter().map(String::as_str).collect();
        let before = crate::for_tests::root_index_builds_for_tests();
        let work_before =
            crate::resolver_core::request_store_view::root_membership_work_for_tests();
        assert!(view.validates_retained_signature(&signature, &roots));
        let work = crate::resolver_core::request_store_view::root_membership_work_for_tests()
            - work_before;
        let indexes = crate::for_tests::root_index_builds_for_tests() - before;
        assert_eq!(indexes, usize::from(size > 7));
        let size = usize::from(size);
        assert!(
            work <= size * (size + 1) / 2,
            "{size} matching roots used {work} operations"
        );
        eprintln!("matching_roots={size} membership={work} indexes={indexes}");
    }
}

/// A fact that will not validate, deep under a chain of consumed results,
/// is read once per applicable context however many of its ancestors later
/// signatures reach it through: the failing walk settles every receipt it
/// proved reaches it, and a later walk stops at the first one it meets. A
/// strict root or a completion that can change what the receipts answer is
/// another context; a sibling the failing walk never finished is settled
/// neither way.
#[test]
fn a_deep_stale_fact_is_read_once_per_applicable_context() {
    let base = base_view();
    let overlay = overlay_through(33);
    let levels = stale_chain(ResultReceipt::new);
    let through = |level: usize| vec![FactVersionRef::Receipt(levels[level].clone())];
    let view = RequestStoreView::new(&base, Arc::clone(&overlay));

    assert_eq!(
        reads(&view, &through(31), &[]),
        (Err(0), 32),
        "the stale fact and every level's own fact above it"
    );
    assert_eq!(view.settled_receipts_for_tests(true), 32);
    for level in 0..32 {
        assert_eq!(
            reads(&view, &through(level), &[]),
            (Err(0), 0),
            "level {level} answers from its settled verdict"
        );
    }

    let sibling = ResultReceipt::new(vec![whole(32)]);
    let consumer = ResultReceipt::new(vec![
        FactVersionRef::Receipt(levels[17].clone()),
        FactVersionRef::Receipt(sibling.clone()),
    ]);
    assert_eq!(
        reads(&view, &[FactVersionRef::Receipt(consumer)], &[]),
        (Err(0), 0),
        "a new consumer of a stale level"
    );
    let sibling = [FactVersionRef::Receipt(sibling)];
    assert_eq!(
        reads(&view, &sibling, &[]),
        (Ok(()), 1),
        "the consumer's valid sibling was never proven stale"
    );
    assert_eq!(reads(&view, &sibling, &[]), (Ok(()), 0));

    // Levels 20 to 31 reach the strict root and are read again, down to the
    // first level below it, whose settled verdict still applies.
    assert_eq!(
        reads(&view, &through(31), &["/r/20.ts"]),
        (Err(0), 12),
        "a strict root the levels above 19 reach"
    );
    assert_eq!(reads(&view, &through(31), &["/r/20.ts"]), (Err(0), 0));
    assert_eq!(
        reads(&view, &through(31), &["/elsewhere.ts"]),
        (Err(0), 0),
        "a strict root no level reaches"
    );

    overlay.insert_whole_hash_for_tests("/moved.ts", [9; 16]);
    assert_eq!(
        reads(&view, &through(31), &[]),
        (Err(0), 32),
        "a completion moved the overlay"
    );
    assert_eq!(reads(&view, &through(31), &[]), (Err(0), 0));
    overlay.insert_whole_hash_for_tests("/r/0.ts", [0xEE; 16]);
    assert_eq!(
        reads(&view, &through(31), &[]),
        (Ok(()), 32),
        "a completion that restores the recorded version validates"
    );

    let fresh = RequestStoreView::new(&base, overlay_through(33));
    assert_eq!(
        reads(&fresh, &through(31), &[]),
        (Err(0), 32),
        "a new request"
    );
}

/// A retained signature answers a repeated probe from its settled verdict,
/// stale as well as valid, without reading its facts again; a borrowed
/// signature, which no cache retains, is read each time.
#[test]
fn a_retained_signature_answers_its_settled_verdict() {
    let base = base_view();
    let overlay = overlay_through(32);
    overlay.insert_whole_hash_for_tests("/stale.ts", [1; 16]);
    let mut facts: Vec<FactVersionRef> = (1..32).map(whole).collect();
    facts.push(FactVersionRef::FileWholeHash {
        canonical_id: "/stale.ts".into(),
        hash: [2; 16],
    });
    let stale: verter_session_query::facts::fact_set::FactSet =
        verter_session_query::facts::policy::FactPolicy::production().set_of(facts);
    let valid: verter_session_query::facts::fact_set::FactSet =
        verter_session_query::facts::policy::FactPolicy::production()
            .set_of((1..32).map(whole).collect::<Vec<_>>());
    let view = RequestStoreView::new(&base, overlay);
    let probe = |signature: &verter_session_query::facts::fact_set::FactSet| {
        let before = view.leaf_validations_for_tests();
        let answer = view.validates_retained_signature(signature, &[]);
        (answer, view.leaf_validations_for_tests() - before)
    };

    assert_eq!(probe(&stale), (false, 32));
    assert_eq!(probe(&stale), (false, 0), "a settled stale signature");
    assert_eq!(probe(&valid), (true, 31));
    assert_eq!(probe(&valid), (true, 0), "a settled valid signature");
    assert_eq!(view.settled_signatures_for_tests(), 2);
    assert_eq!(
        reads(&view, &stale.to_vec(), &[]),
        (Err(31), 32),
        "the same facts borrowed"
    );
}

/// Only a failure the view would answer the same way again is negative
/// validity. A fact read from a store live writers share fails without a
/// verdict and is read again; a validation the overlay moved under, or one
/// that met a completion writer in flight, settles nothing; a base not
/// proven current refuses without reading or settling anything.
#[test]
fn uncertain_torn_and_refused_failures_settle_nothing() {
    use verter_session_query::facts::fact_cache::RouteSurfaceFactRef;
    use verter_session_query::facts::registry::FactLane;
    use verter_session_query::facts::FactKey;

    let base = base_view();
    let levels = stale_chain(ResultReceipt::new);
    let deep = vec![FactVersionRef::Receipt(levels[31].clone())];

    let live = ResultReceipt::new(vec![
        FactVersionRef::RouteSurface(RouteSurfaceFactRef {
            canonical_id: "/r/1.ts".into(),
            key: FactKey::ProgramFileOrder {
                program: Arc::from("/unpublished/tsconfig.json"),
            },
            lane: FactLane::Semantic,
            expected_hash: [3; 16],
        }),
        whole(1),
    ]);
    let live = [FactVersionRef::Receipt(live)];
    let view = RequestStoreView::new(&base, overlay_through(33));
    for _ in 0..2 {
        let (answer, read) = reads(&view, &live, &[]);
        assert!(answer.is_err());
        assert!(read > 0, "a live-store failure is read again");
    }
    assert_eq!(view.settled_receipts_for_tests(true), 0);

    let moving = overlay_through(33);
    let mover = Arc::clone(&moving);
    let torn = RequestStoreView::new(&base, moving).with_validation_step_hook_for_tests(Arc::new(
        move |step| {
            if step == 0 {
                mover.insert_whole_hash_for_tests("/moved.ts", [9; 16]);
            }
        },
    ));
    assert!(torn.validate_fact_signature((&deep).into(), &[]).is_err());
    assert_eq!(
        (
            torn.settled_receipts_for_tests(true),
            torn.settled_receipts_for_tests(false)
        ),
        (0, 0),
        "a validation the overlay moved under settles nothing"
    );

    let writing = overlay_through(33);
    let in_flight = RequestStoreView::new(&base, Arc::clone(&writing));
    let (entered_tx, entered_rx) = std::sync::mpsc::sync_channel(0);
    let (release_tx, release_rx) = std::sync::mpsc::sync_channel(0);
    let writer = std::thread::spawn(move || {
        writing.hold_revision_in_flight_for_tests(entered_tx, release_rx);
    });
    entered_rx.recv().expect("writer must enter its bracket");
    let answer = reads(&in_flight, &deep, &[]);
    release_tx.send(()).expect("writer must still be waiting");
    writer.join().expect("writer must finish");
    assert_eq!(answer, (Err(0), 0), "a writer in flight refuses unread");
    assert_eq!(in_flight.settled_receipts_for_tests(true), 0);
    assert_eq!(
        reads(&in_flight, &deep, &[]),
        (Err(0), 32),
        "the restored state reads the chain"
    );

    let unproven = RequestStoreView::new_cold_seed(&base, overlay_through(33), false);
    assert_eq!(reads(&unproven, &deep, &[]), (Err(0), 0));
    let retained: verter_session_query::facts::fact_set::FactSet =
        verter_session_query::facts::policy::FactPolicy::production().set_of(deep);
    assert!(!unproven.validates_retained_signature(&retained, &[]));
    assert_eq!(
        (
            unproven.settled_receipts_for_tests(true),
            unproven.settled_signatures_for_tests()
        ),
        (0, 0),
        "an unproven base settles nothing"
    );
}

/// A fact whose read was refused its lease proves nothing about the fact: it
/// fails without a verdict, for it or for the receipts that reach it, and a
/// probe once the lease can be had reads it again. The function-program
/// index walk is refused its walk-stack lease once (the fault is armed for
/// this source's reservation size alone, and the source is forced onto the
/// region path so the walk reserves on any thread's stack); the retry
/// through the same view at the same completion state validates.
#[test]
fn a_refused_index_walk_settles_nothing_and_its_retry_validates() {
    use oxc_span::SourceType;
    use verter_parser::oxc_parse::faults::{
        fail_reservations_needing, force_reservations, reservations_needing, Reservation,
    };
    use verter_session_query::facts::fact_cache::{
        ProgramAnalysisFactRef, ProgramAnalysisFunctionRef,
    };

    const ID: &str = "/ws/refused_flow.ts";
    // A nesting no other test in this binary forces, so the armed fault
    // reaches this source's walk alone.
    let source = format!(
        "export function alpha(n: number) {{\n  return {}n{};\n}}\n",
        "(".repeat(73),
        ")".repeat(73)
    );
    let upsert = |host: &VerterHost| {
        let _ = host
            .upsert(UpsertRequest {
                canonical_id: Some(ID.to_string()),
                input_id: ID.to_string(),
                source: Arc::from(source.as_str()),
                file_language: FileLanguage::script_ts(),
                aliases: Vec::new(),
            })
            .expect("upsert must succeed");
    };
    let function = ProgramAnalysisFunctionRef {
        canonical_id: verter_type_expr::OwnedStr::from(ID),
        owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
        merged_symbol_name: verter_type_expr::OwnedStr::from("alpha"),
        symbol_space: verter_session_query::facts::SymbolSpace::Value,
        function_part: verter_type_expr::facts::FunctionPartIdentity::DeclarationBody,
        overload_ordinal: 0,
    };
    let live_hash = {
        let host = VerterHost::new_standalone(HostConfig::default());
        upsert(&host);
        host.ensure_indexed_ready(ID)
            .expect("indexed")
            .shallow_state
            .decl_bodies()
            .function_program_index()
            .value
            .value_function(
                function.owner,
                "alpha",
                &function.function_part,
                function.overload_ordinal,
            )
            .expect("alpha is indexed")
            .entry()
            .flow_body_stable_hash()
    };
    let needed = verter_parser::oxc_parse::parse_stack_bytes(&source, SourceType::ts());
    let _forcing = force_reservations(&[Reservation::Lease], needed);
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert(&host);
    host.ensure_indexed_ready(ID).expect("indexed");
    let base = host.resolver_store_view_read().into_owned_view();
    let receipt = ResultReceipt::new(vec![
        FactVersionRef::ProgramAnalysis(ProgramAnalysisFactRef::FlowBody {
            function,
            flow_body_stable_hash: live_hash,
        }),
        whole(1),
    ]);
    let signature = [FactVersionRef::Receipt(receipt)];
    let view = RequestStoreView::new(&base, overlay_through(2));

    let leases = reservations_needing(Reservation::Lease, needed);
    fail_reservations_needing(Reservation::Lease, needed, 1);
    let refused = reads(&view, &signature, &[]);
    fail_reservations_needing(Reservation::Lease, needed, 0);
    assert!(
        reservations_needing(Reservation::Lease, needed) > leases,
        "the index walk asked for its lease"
    );
    assert_eq!(refused, (Err(0), 2), "the refused walk fails the receipt");
    assert_eq!(
        view.settled_receipts_for_tests(true),
        0,
        "a refused read is not negative validity"
    );
    assert_eq!(
        reads(&view, &signature, &[]),
        (Ok(()), 2),
        "the retry reads the fact again and validates"
    );
    assert_eq!(reads(&view, &signature, &[]), (Ok(()), 0));
}

/// A parse fact whose lazy declaration-body read misses its lease reads as
/// absent without being absent: it fails, settles nothing, and is read
/// again. That holds for the zero sentinel too: the refused read is not the
/// consistent absence the sentinel records, so neither the request view nor
/// the base view it reads through accepts it.
#[test]
fn a_parse_fact_whose_body_read_misses_its_lease_settles_nothing() {
    use verter_session_query::facts::fact_cache::ParseFactRef;
    use verter_session_query::facts::registry::{FactLane, InternedName};
    use verter_session_query::facts::{FactKey, SymbolSpace};

    const ID: &str = "/ws/lease_missed.ts";
    let host = VerterHost::new_standalone(HostConfig::default());
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(ID.to_string()),
            input_id: ID.to_string(),
            source: Arc::from(
                "export type A = { x: number };
export type B = { y: string };
",
            ),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("upsert must succeed");
    let indexed = host.ensure_indexed_ready(ID).expect("indexed");
    assert!(
        indexed.shallow_state.type_decl("A").is_some(),
        "A lowers under a live lease, pinning the retained snapshot"
    );
    indexed
        .shallow_state
        .decl_bodies()
        .release_retained_snapshot_for_test();
    let base = host.resolver_store_view_read().into_owned_view();
    for expected_hash in [[7; 16], [0; 16]] {
        let fact = ParseFactRef {
            canonical_id: ID.into(),
            key: FactKey::Export {
                name: InternedName(Arc::from("B")),
                space: SymbolSpace::Type,
            },
            lane: FactLane::Semantic,
            expected_hash,
        };
        assert!(
            !base.validates_parse_domain(&fact),
            "the base view rejects the refused read of {expected_hash:?}"
        );
        let signature = [FactVersionRef::Receipt(ResultReceipt::new(vec![
            FactVersionRef::Parse(fact),
        ]))];
        let view = RequestStoreView::new(&base, overlay_through(1));
        for probe in 0..2 {
            assert_eq!(
                reads(&view, &signature, &[]),
                (Err(0), 1),
                "probe {probe} reads the lease-missed fact recorded as {expected_hash:?}"
            );
        }
        assert_eq!(view.settled_receipts_for_tests(true), 0);
    }
}

/// A flow-body fact whose function-program index walk finds its retained
/// snapshot's lease pin broken reads an empty index that proves nothing
/// about the function: it fails, settles nothing, and is read again.
#[test]
fn a_flow_body_fact_whose_index_walk_misses_its_lease_settles_nothing() {
    use verter_session_query::facts::fact_cache::{
        ProgramAnalysisFactRef, ProgramAnalysisFunctionRef,
    };

    const ID: &str = "/ws/lease_missed_flow.ts";
    const SOURCE: &str = "export type A = { x: number };
export function alpha(n: number) {
  return n;
}
";
    let upsert = |host: &VerterHost| {
        let _ = host
            .upsert(UpsertRequest {
                canonical_id: Some(ID.to_string()),
                input_id: ID.to_string(),
                source: Arc::from(SOURCE),
                file_language: FileLanguage::script_ts(),
                aliases: Vec::new(),
            })
            .expect("upsert must succeed");
    };
    let function = ProgramAnalysisFunctionRef {
        canonical_id: verter_type_expr::OwnedStr::from(ID),
        owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
        merged_symbol_name: verter_type_expr::OwnedStr::from("alpha"),
        symbol_space: verter_session_query::facts::SymbolSpace::Value,
        function_part: verter_type_expr::facts::FunctionPartIdentity::DeclarationBody,
        overload_ordinal: 0,
    };
    let live_hash = {
        let host = VerterHost::new_standalone(HostConfig::default());
        upsert(&host);
        host.ensure_indexed_ready(ID)
            .expect("indexed")
            .shallow_state
            .decl_bodies()
            .function_program_index()
            .value
            .value_function(
                function.owner,
                "alpha",
                &function.function_part,
                function.overload_ordinal,
            )
            .expect("alpha is indexed")
            .entry()
            .flow_body_stable_hash()
    };
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert(&host);
    let indexed = host.ensure_indexed_ready(ID).expect("indexed");
    assert!(
        indexed.shallow_state.type_decl("A").is_some(),
        "A lowers under a live lease, pinning the retained snapshot"
    );
    indexed
        .shallow_state
        .decl_bodies()
        .release_retained_snapshot_for_test();
    let base = host.resolver_store_view_read().into_owned_view();
    let signature = [FactVersionRef::Receipt(ResultReceipt::new(vec![
        FactVersionRef::ProgramAnalysis(ProgramAnalysisFactRef::FlowBody {
            function,
            flow_body_stable_hash: live_hash,
        }),
    ]))];
    let view = RequestStoreView::new(&base, overlay_through(1));
    for probe in 0..2 {
        assert_eq!(
            reads(&view, &signature, &[]),
            (Err(0), 1),
            "probe {probe} reads the lease-missed flow-body fact"
        );
    }
    assert_eq!(
        view.settled_receipts_for_tests(true),
        0,
        "a broken lease pin is not negative validity"
    );
}

/// Settled verdicts belong to the request: they keep the evidence they
/// name alive while the view lives and release every byte of it with the
/// view.
#[test]
fn settled_verdicts_release_their_evidence_with_the_view() {
    use verter_session_query::retention::{RetentionLimits, SemanticRetentionAccount};

    let account = SemanticRetentionAccount::new(RetentionLimits::default());
    let base = base_view();
    let view = RequestStoreView::new(&base, overlay_through(33));
    {
        let levels = stale_chain(|facts| ResultReceipt::new_on(facts, &account));
        let valid = ResultReceipt::new_on(vec![whole(32)], &account);
        let retained: verter_session_query::facts::fact_set::FactSet =
            verter_session_query::facts::policy::FactPolicy::production().set_of(vec![
                FactVersionRef::Receipt(levels[31].clone()),
                FactVersionRef::Receipt(valid.clone()),
            ]);
        assert!(view
            .validate_fact_signature(
                verter_session_query::facts::fact_set::FactsRef::from(&[FactVersionRef::Receipt(
                    valid
                )]),
                &[]
            )
            .is_ok());
        assert!(!view.validates_retained_signature(&retained, &[]));
        assert_eq!(view.settled_receipts_for_tests(true), 32);
        assert_eq!(view.settled_receipts_for_tests(false), 1);
    }
    assert!(
        account.snapshot().pinned_bytes > 0,
        "the verdicts hold their evidence for the request"
    );
    drop(view);
    assert_eq!(
        account.snapshot().pinned_bytes,
        0,
        "the request's verdicts drain with it"
    );
}
