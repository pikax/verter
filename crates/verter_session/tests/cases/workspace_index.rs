//! The workspace candidate index through its real host route: bounded
//! enumeration, certification, incremental equivalence and the public status of
//! every answer. The crate-private writer is covered beside it in the crate.

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use verter_identity::identity::ContentId;
use verter_session::workspace_index::*;
use verter_session::*;
use verter_session_query::analysis::types::AnalyzedBindingKind;

fn never() -> impl Fn() -> bool {
    || false
}

fn upsert_file(host: &VerterHost, id: &str, source: &str, language: FileLanguage) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: id.to_string(),
            source: Arc::from(source),
            file_language: language,
            aliases: Vec::new(),
        })
        .expect("upsert");
}

const WIDE: QueryBudgets = QueryBudgets {
    postings: 1000,
    records: 1000,
};

fn vue_source(variable: &str) -> String {
    format!(
        "<script setup>\nconst el = document.body;\nel.style.setProperty('{variable}', 'red');\n</script>\n<template><div :style=\"{{ '{variable}': 'blue' }}\">A</div></template>\n<style>\n:root {{ {variable}: blue; }}\n.a {{ color: var({variable}); }}\n</style>"
    )
}

const SVELTE_SOURCE: &str =
    "<script>\nlet a = 1;\n</script>\n<div>{a}</div>\n<style>\n:global(html) { --brand: blue; }\n.b { color: var(--brand); }\n</style>";

/// Everything an answer claims about a candidate, in answer order.
fn fingerprint(page: &CandidatePage) -> Vec<String> {
    page.candidates
        .iter()
        .map(|candidate| {
            let record = candidate.record().expect("candidate record");
            format!(
                "{}|{:?}|{:?}|{:?}|{:?}|{:?}",
                candidate.contribution.basis.locator,
                record.slot,
                record.subject,
                record.row,
                record.origins,
                record.quality
            )
        })
        .collect()
}

fn css_keys(name: &str) -> Lookup {
    Lookup::Keys(vec![PostingKey::exact(PartitionFamily::CssVariables, name)])
}

fn selector_keys(name: &str) -> Lookup {
    Lookup::Keys(vec![PostingKey::exact(
        PartitionFamily::StyleSelectors,
        name,
    )])
}

fn css_demand() -> IndexDemand {
    IndexDemand::of([IndexDemandPartition::CssVariables])
}

#[test]
fn css_var_flow_reports_each_surface_across_vue_and_svelte() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_file(
        &host,
        "/src/A.vue",
        &vue_source("--brand"),
        FileLanguage::vue(),
    );
    upsert_file(
        &host,
        "/src/B.svelte",
        SVELTE_SOURCE,
        FileLanguage::svelte(),
    );

    let flow = host.css_var_flow("--brand", None).flow;
    let paths = |list: &[Arc<std::path::Path>]| {
        let mut paths: Vec<String> = list.iter().map(|p| p.display().to_string()).collect();
        paths.sort();
        paths
    };
    assert_eq!(
        paths(&flow.style_definitions),
        ["/src/A.vue", "/src/B.svelte"]
    );
    assert_eq!(
        paths(&flow.style_var_usages),
        ["/src/A.vue", "/src/B.svelte"]
    );
    assert_eq!(paths(&flow.template_definitions), ["/src/A.vue"]);
    assert_eq!(paths(&flow.script_manipulations), ["/src/A.vue"]);
}

#[test]
fn incremental_answers_equal_a_fresh_host_across_edit_revert_add_and_delete() {
    let incremental = VerterHost::new_standalone(HostConfig::default());
    let mut files: BTreeMap<&str, String> = BTreeMap::new();
    let vue = |host: &VerterHost, id: &str, source: &str| {
        upsert_file(host, id, source, FileLanguage::vue());
    };

    let assert_equal = |incremental: &VerterHost, files: &BTreeMap<&str, String>, step: &str| {
        let fresh = VerterHost::new_standalone(HostConfig::default());
        // Reverse load order: answers must not depend on it.
        for (id, source) in files.iter().rev() {
            vue(&fresh, id, source);
        }
        for name in ["--brand", "--other"] {
            let demand = css_demand();
            let (live, _) = incremental.workspace_index_candidates(
                &demand,
                css_keys(name),
                WIDE,
                None,
                &never(),
            );
            let (clean, _) =
                fresh.workspace_index_candidates(&demand, css_keys(name), WIDE, None, &never());
            assert_eq!(fingerprint(&live), fingerprint(&clean), "{step}: {name}");
            assert_eq!(live.status, clean.status, "{step}: {name}");
            assert_eq!(live.status, EnumerationStatus::Complete, "{step}: {name}");
        }
    };

    files.insert("/src/A.vue", vue_source("--brand"));
    files.insert("/src/B.vue", vue_source("--other"));
    for (id, source) in &files {
        vue(&incremental, id, source);
    }
    assert_equal(&incremental, &files, "initial");

    files.insert("/src/A.vue", vue_source("--other"));
    vue(&incremental, "/src/A.vue", &vue_source("--other"));
    assert_equal(&incremental, &files, "edit");

    files.insert("/src/A.vue", vue_source("--brand"));
    vue(&incremental, "/src/A.vue", &vue_source("--brand"));
    assert_equal(&incremental, &files, "revert");

    files.insert("/src/C.vue", vue_source("--brand"));
    vue(&incremental, "/src/C.vue", &vue_source("--brand"));
    assert_equal(&incremental, &files, "add");

    files.remove("/src/C.vue");
    incremental.remove("/src/C.vue");
    assert_equal(&incremental, &files, "delete");
}

#[test]
fn a_warm_request_visits_no_file_and_an_edit_projects_only_that_file() {
    let host = VerterHost::new_standalone(HostConfig::default());
    for id in ["/src/A.vue", "/src/B.vue", "/src/C.vue"] {
        upsert_file(&host, id, &vue_source("--brand"), FileLanguage::vue());
    }
    let demand = css_demand();
    let (_, cold) =
        host.workspace_index_candidates(&demand, css_keys("--brand"), WIDE, None, &never());
    assert_eq!(cold.projections, 3);

    let (warm_page, warm) =
        host.workspace_index_candidates(&demand, css_keys("--brand"), WIDE, None, &never());
    assert_eq!(
        warm,
        IndexReconcileWork::default(),
        "warm does no file work"
    );
    assert_eq!(warm_page.status, EnumerationStatus::Complete);

    upsert_file(
        &host,
        "/src/B.vue",
        &vue_source("--other"),
        FileLanguage::vue(),
    );
    let (_, edited) =
        host.workspace_index_candidates(&demand, css_keys("--brand"), WIDE, None, &never());
    assert_eq!(edited.projections, 1, "only the edited file");
    assert_eq!(edited.unchanged, 0, "no other slot is visited");
    assert_eq!(
        edited.population_checks, 1,
        "the committed mutation names the one file to re-read"
    );
}

#[test]
fn growing_unrelated_files_adds_no_candidate_visits() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_file(
        &host,
        "/src/A.vue",
        &vue_source("--brand"),
        FileLanguage::vue(),
    );
    let demand = css_demand();
    let (small, _) =
        host.workspace_index_candidates(&demand, css_keys("--brand"), WIDE, None, &never());
    for file in 0..8 {
        let id = format!("/src/Unrelated{file}.vue");
        upsert_file(&host, &id, &vue_source("--other"), FileLanguage::vue());
    }
    let (grown, reconcile) =
        host.workspace_index_candidates(&demand, css_keys("--brand"), WIDE, None, &never());
    assert_eq!(grown.work, small.work, "lookups and visits stay fixed");
    assert_eq!(reconcile.projections, 8, "each new file is projected once");
}

#[test]
fn an_undemanded_partition_is_never_projected() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_file(
        &host,
        "/src/A.vue",
        &vue_source("--brand"),
        FileLanguage::vue(),
    );
    let (snapshot, status, work) = host.workspace_index_reconcile(&css_demand(), &never());
    assert_eq!(status, ReconcileStatus::Current);
    assert_eq!(work.projections, 1);
    assert_eq!(snapshot.contribution_count(), 1);
    for surface in IndexSurface::ALL
        .into_iter()
        .filter(|surface| *surface != IndexSurface::CssVariables)
    {
        assert_eq!(
            snapshot.contributions_in_partition(&surface.id()).count(),
            0,
            "{surface:?}"
        );
    }
}

#[test]
fn a_cancelled_reconcile_warms_nothing_and_a_retry_equals_fresh() {
    let host = VerterHost::new_standalone(HostConfig::default());
    for id in ["/src/A.vue", "/src/B.vue"] {
        upsert_file(&host, id, &vue_source("--brand"), FileLanguage::vue());
    }
    let demand = css_demand();
    let calls = Cell::new(0);
    let cancel_mid_walk = || {
        calls.set(calls.get() + 1);
        calls.get() == 2
    };
    let (page, _) =
        host.workspace_index_candidates(&demand, css_keys("--brand"), WIDE, None, &cancel_mid_walk);
    assert_eq!(page.status, EnumerationStatus::Cancelled);
    assert!(page.candidates.is_empty());
    let store = host.project_type_store().workspace_index().snapshot();
    assert_eq!(
        store.contribution_count(),
        0,
        "no staged prefix was published"
    );
    assert!(store.coverage(&demand.scope()).is_none());

    let (retry, retried) =
        host.workspace_index_candidates(&demand, css_keys("--brand"), WIDE, None, &never());
    assert_eq!(retry.status, EnumerationStatus::Complete);
    assert_eq!(retried.projections, 2);

    let fresh = VerterHost::new_standalone(HostConfig::default());
    for id in ["/src/B.vue", "/src/A.vue"] {
        upsert_file(&fresh, id, &vue_source("--brand"), FileLanguage::vue());
    }
    let (clean, _) =
        fresh.workspace_index_candidates(&demand, css_keys("--brand"), WIDE, None, &never());
    assert_eq!(fingerprint(&retry), fingerprint(&clean));
}

#[test]
fn a_host_budget_shortfall_is_partial_and_certifies_nothing_about_the_rest() {
    let host = VerterHost::new_standalone(HostConfig::default());
    for id in ["/src/A.vue", "/src/B.vue"] {
        upsert_file(&host, id, &vue_source("--brand"), FileLanguage::vue());
    }
    let tight = QueryBudgets {
        postings: 1,
        records: 1,
    };
    let answer = host.css_var_candidates("--brand", tight, None, &never());
    assert_eq!(answer.page.candidates.len(), 1);
    assert!(matches!(
        answer.page.status,
        EnumerationStatus::Partial {
            cause: PartialCause::BudgetExhausted,
            continuation: Some(_),
            ..
        }
    ));
}

#[test]
fn component_uses_answer_by_tag_and_no_name_finds_a_view_the_owner_never_produced() {
    let sources = [
        (
            "/a/Card.vue",
            "<template><Badge /><Badge /></template>",
            FileLanguage::vue(),
        ),
        ("/b/Card.svelte", "<div>card</div>", FileLanguage::svelte()),
        (
            "/c/Badge.vue",
            "<template><div /></template>",
            FileLanguage::vue(),
        ),
    ];
    let demand = IndexDemand::of([IndexDemandPartition::Components]);
    let load = |order: &[usize]| {
        let host = VerterHost::new_standalone(HostConfig::default());
        for &position in order {
            let (id, source, language) = &sources[position];
            upsert_file(&host, id, source, language.clone());
        }
        host
    };
    let answer = |host: &VerterHost, name: &str| {
        let keys = Lookup::Keys(vec![PostingKey::exact(PartitionFamily::Components, name)]);
        host.workspace_index_candidates(&demand, keys, WIDE, None, &never())
            .0
    };

    let host = load(&[0, 1, 2]);
    // A basename finds nothing: it never identified a component.
    let cards = answer(&host, "Card");
    assert_eq!(cards.status, EnumerationStatus::Complete);
    assert!(cards.candidates.is_empty());

    // Two uses of one tag are two occurrences, each still unresolved. Choosing
    // the target stays with the owner.
    let badges = answer(&host, "Badge");
    assert_eq!(badges.status, EnumerationStatus::Complete);
    assert_eq!(badges.candidates.len(), 2, "uses only, no file named Badge");
    assert!(badges.candidates.iter().all(|candidate| {
        let record = candidate.record().expect("record");
        record.quality == FactQuality::Unresolved
            && matches!(
                &record.row,
                IndexRow::Component(ComponentRow::Usage {
                    target: Endpoint::Unresolved { .. },
                    ..
                })
            )
    }));
    assert_eq!(
        fingerprint(&badges),
        fingerprint(&answer(&load(&[2, 1, 0]), "Badge"))
    );

    // Each component subject carries a typed marker of the view nobody
    // produced, under its unit, and no exact view row exists.
    let snapshot = host.project_type_store().workspace_index().snapshot();
    let mut markers = BTreeSet::new();
    for slot in snapshot.contributions_in_partition(&IndexSurface::ComponentInventory.id()) {
        for record in &slot.records {
            assert!(!matches!(
                record.row,
                IndexRow::Component(ComponentRow::View { .. })
            ));
            if let IndexRow::Component(ComponentRow::ViewUnavailable { cause }) = &record.row {
                assert_eq!(*cause, FactCause::OwnerUnavailable);
                assert_eq!(record.quality, FactQuality::Unresolved);
                markers.insert(slot.basis.locator.to_string());
                let by_unit = PostingKey::forward(
                    PartitionFamily::Components,
                    &slot.basis.unit.digest().to_hex(),
                );
                assert_eq!(record.keys, vec![by_unit]);
            }
        }
    }
    assert_eq!(
        markers,
        ["/a/Card.vue", "/b/Card.svelte", "/c/Badge.vue"]
            .map(String::from)
            .into_iter()
            .collect(),
        "one basename across carriers stays two subjects"
    );
}

#[test]
fn name_search_returns_similarly_named_candidates_without_choosing_a_target() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let source = |name: &str| format!("<script setup>\nconst {name} = 1;\n</script>");
    upsert_file(&host, "/src/A.vue", &source("Total"), FileLanguage::vue());
    upsert_file(
        &host,
        "/src/B.svelte",
        &source("total"),
        FileLanguage::svelte(),
    );
    upsert_file(
        &host,
        "/src/C.vue",
        &source("subtotal"),
        FileLanguage::vue(),
    );
    let demand = IndexDemand::of([IndexDemandPartition::Bindings]);
    let search = Lookup::Prefix {
        partition: PartitionFamily::Bindings,
        family: KeyFamily::Lexical,
        version: LEXICAL_PROJECTION_VERSION,
        prefix: Arc::from("total"),
    };
    let (page, _) = host.workspace_index_candidates(&demand, search, WIDE, None, &never());
    let mut spellings: Vec<String> = page
        .candidates
        .iter()
        .filter_map(|candidate| match &candidate.record()?.row {
            IndexRow::Name(row) => Some(row.spelling.to_string()),
            _ => None,
        })
        .collect();
    spellings.sort();
    assert_eq!(
        spellings,
        ["Total", "total"],
        "both stay candidates; prefix excludes subtotal"
    );
    assert_eq!(page.status, EnumerationStatus::Complete);
    assert!(page.candidates.iter().all(|candidate| matches!(
        candidate.record().map(|record| &record.row),
        Some(IndexRow::Name(NameRow {
            declaration_kind: Some(AnalyzedBindingKind::Const),
            ..
        }))
    )));

    // The query prefix is folded the way the postings were.
    let folded = |prefix: &str| {
        let search = Lookup::Prefix {
            partition: PartitionFamily::Bindings,
            family: KeyFamily::Lexical,
            version: LEXICAL_PROJECTION_VERSION,
            prefix: Arc::from(prefix),
        };
        host.workspace_index_candidates(&demand, search, WIDE, None, &never())
            .0
    };
    assert_eq!(fingerprint(&folded("TOTAL")), fingerprint(&page));
    assert_eq!(folded("TOTAL").status, EnumerationStatus::Complete);

    // A projection no producer posts under is unsupported, never an empty
    // population: neither a future lexical version nor a versioned key family
    // that has no projection.
    for (family, version) in [
        (KeyFamily::Lexical, LEXICAL_PROJECTION_VERSION + 1),
        (KeyFamily::Lexical, 0),
        (KeyFamily::Exact, 1),
    ] {
        let search = Lookup::Prefix {
            partition: PartitionFamily::Bindings,
            family,
            version,
            prefix: Arc::from("total"),
        };
        let (page, _) = host.workspace_index_candidates(&demand, search, WIDE, None, &never());
        assert!(page.candidates.is_empty(), "{family:?} v{version}");
        assert!(
            matches!(
                page.status,
                EnumerationStatus::Partial {
                    cause: PartialCause::UnsupportedCoverage,
                    continuation: None,
                    ..
                }
            ),
            "{family:?} v{version}: {:?}",
            page.status
        );
    }
}

#[test]
fn imports_stay_explicitly_unresolved_with_their_cause() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_file(
        &host,
        "/src/A.vue",
        "<script setup>\nimport x from './missing';\nconsole.log(x);\n</script>",
        FileLanguage::vue(),
    );
    let demand = IndexDemand::of([IndexDemandPartition::Imports]);
    let keys = Lookup::Keys(vec![PostingKey::reverse(
        PartitionFamily::Imports,
        "./missing",
    )]);
    let (page, _) = host.workspace_index_candidates(&demand, keys, WIDE, None, &never());
    let [candidate] = page.candidates.as_slice() else {
        panic!("one import edge, got {}", page.candidates.len());
    };
    let record = candidate.record().expect("record");
    assert_eq!(record.quality, FactQuality::Unresolved);
    let IndexRow::Link(link) = &record.row else {
        panic!("a link row");
    };
    assert_eq!(link.role, LinkRole::Unresolved);
    assert!(link.resolution.is_none());
    assert!(matches!(
        link.target,
        Endpoint::Unresolved {
            cause: FactCause::SpecifierUnresolved,
            ..
        }
    ));
}

#[test]
fn stylesheet_scopes_stay_producer_facts_across_vue_and_svelte() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_file(
        &host,
        "/src/A.vue",
        "<template><div /></template>\n<style scoped>\n.same { color: red; }\n</style>\n<style module>\n.same { color: red; }\n</style>\n<style>\n.same { color: red; }\n</style>",
        FileLanguage::vue(),
    );
    upsert_file(
        &host,
        "/src/B.svelte",
        "<div class=\"same\">b</div>
<style>
.same { color: red; }
</style>",
        FileLanguage::svelte(),
    );
    let demand = IndexDemand::of([IndexDemandPartition::StyleSelectors]);
    let (page, _) =
        host.workspace_index_candidates(&demand, selector_keys("same"), WIDE, None, &never());
    let locators: BTreeSet<&str> = page
        .candidates
        .iter()
        .map(|candidate| candidate.contribution.basis.locator.as_ref())
        .collect();
    assert_eq!(
        locators,
        ["/src/A.vue", "/src/B.svelte"].into_iter().collect(),
        "both carriers answer through the one partition"
    );
    let mut by_block: Vec<(u32, StyleScope)> = page
        .candidates
        .iter()
        .filter(|candidate| candidate.contribution.basis.locator.as_ref() == "/src/A.vue")
        .filter_map(|candidate| match &candidate.record()?.row {
            IndexRow::Stylesheet(StylesheetRow {
                surface: StyleSurface::Style { block },
                scope,
                ..
            }) => Some((*block, *scope)),
            _ => None,
        })
        .collect();
    by_block.sort_by_key(|(block, _)| *block);
    assert_eq!(
        by_block,
        [
            (0, StyleScope::Scoped),
            (1, StyleScope::Module),
            (2, StyleScope::Global)
        ]
    );
}

#[test]
fn origins_are_exact_authored_bytes_past_non_ascii_text_and_explicit_when_absent() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let source = "<template><div :style=\"{ '--brand': 'x' }\" /></template>\n<style>\n/* héllo 日本語 */\n:root { --brand: blue; }\n</style>";
    upsert_file(&host, "/src/A.vue", source, FileLanguage::vue());
    let (page, _) =
        host.workspace_index_candidates(&css_demand(), css_keys("--brand"), WIDE, None, &never());
    let mut style_origin = None;
    let mut template_origin = None;
    for candidate in &page.candidates {
        let record = candidate.record().expect("record");
        let IndexRow::Stylesheet(row) = &record.row else {
            continue;
        };
        match row.surface {
            StyleSurface::Style { .. } => style_origin = Some(record.origins.clone()),
            StyleSurface::Template => template_origin = Some(record.origins.clone()),
            StyleSurface::Script => {}
        }
    }
    let style_origin = style_origin.expect("style definition");
    let [RecordOrigin::Authored(anchor)] = style_origin.as_slice() else {
        panic!("expected one authored origin, got {style_origin:?}");
    };
    let start = usize::try_from(anchor.range.start().get()).unwrap();
    let end = usize::try_from(anchor.range.end().get()).unwrap();
    assert_eq!(&source[start..end], "--brand");
    assert_eq!(
        anchor.content,
        ContentId::from_content_bytes(source.as_bytes())
    );
    assert_eq!(
        template_origin.expect("template binding"),
        vec![RecordOrigin::Unavailable(
            OriginUnavailable::OwnerRecordsNoSpan
        )],
        "no fabricated zero range"
    );
}

#[test]
fn typescript_files_join_the_certified_population() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_file(
        &host,
        "/src/theme.ts",
        "document.body.style.setProperty('--brand', 'red');\n",
        FileLanguage::script_ts(),
    );
    upsert_file(
        &host,
        "/src/A.vue",
        &vue_source("--other"),
        FileLanguage::vue(),
    );
    let answer = host.css_var_candidates("--brand", WIDE, None, &never());
    assert_eq!(answer.page.status, EnumerationStatus::Complete);
    assert_eq!(answer.flow.script_manipulations.len(), 1);
    let again = host.css_var_candidates("--brand", WIDE, None, &never());
    assert_eq!(again.work, IndexReconcileWork::default());
}

#[test]
fn deleting_a_file_retires_its_slots_and_changing_its_language_retires_the_old_claim() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_file(
        &host,
        "/src/A.vue",
        &vue_source("--brand"),
        FileLanguage::vue(),
    );
    upsert_file(
        &host,
        "/src/B.vue",
        &vue_source("--brand"),
        FileLanguage::vue(),
    );
    let demand = css_demand();
    let (first, _) =
        host.workspace_index_candidates(&demand, css_keys("--brand"), WIDE, None, &never());
    let per_file = first.candidates.len() / 2;

    host.remove("/src/B.vue");
    let (after, work) =
        host.workspace_index_candidates(&demand, css_keys("--brand"), WIDE, None, &never());
    assert_eq!(work.retired, 1);
    assert_eq!(after.status, EnumerationStatus::Complete);
    assert_eq!(after.candidates.len(), per_file);
    assert!(after
        .candidates
        .iter()
        .all(|candidate| candidate.contribution.basis.locator.as_ref() == "/src/A.vue"));

    // The same path now claims another language row: a plain script has no
    // style or template surface, and the carrier's slot leaves.
    upsert_file(
        &host,
        "/src/A.vue",
        "export const a = 1;\n",
        FileLanguage::script_ts(),
    );
    let (moved, moved_work) =
        host.workspace_index_candidates(&demand, css_keys("--brand"), WIDE, None, &never());
    assert_eq!(moved_work.retired, 1, "the old claim's slot leaves");
    assert_eq!(moved.status, EnumerationStatus::Complete);
    let snapshot = host.project_type_store().workspace_index().snapshot();
    assert_eq!(
        snapshot.contribution_count(),
        1,
        "no parallel claim stays current"
    );
}

#[test]
fn a_file_edited_during_the_walk_cannot_be_certified_and_the_retry_sees_the_edit() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_file(
        &host,
        "/src/A.vue",
        &vue_source("--brand"),
        FileLanguage::vue(),
    );
    upsert_file(
        &host,
        "/src/B.vue",
        &vue_source("--brand"),
        FileLanguage::vue(),
    );
    let demand = css_demand();
    let (first, _) =
        host.workspace_index_candidates(&demand, css_keys("--brand"), WIDE, None, &never());
    assert_eq!(first.status, EnumerationStatus::Complete);

    // Force a second walk, then edit a file the walk has already read as
    // unchanged. The edit lands after capture and before publication.
    upsert_file(
        &host,
        "/src/C.vue",
        &vue_source("--other"),
        FileLanguage::vue(),
    );
    let edited = Cell::new(false);
    let edit_mid_walk = || {
        if !edited.replace(true) {
            upsert_file(
                &host,
                "/src/B.vue",
                &vue_source("--other"),
                FileLanguage::vue(),
            );
        }
        false
    };
    let (page, work) =
        host.workspace_index_candidates(&demand, css_keys("--brand"), WIDE, None, &edit_mid_walk);
    assert_eq!(
        work.publish_attempts, 2,
        "the fence refused the first attempt"
    );
    assert_eq!(page.status, EnumerationStatus::Complete);
    assert!(
        page.candidates
            .iter()
            .all(|candidate| candidate.contribution.basis.locator.as_ref() == "/src/A.vue"),
        "the edited file no longer defines the variable"
    );
}

#[test]
fn a_multi_partition_demand_certifies_its_own_population_and_a_narrower_demand_stays_separate() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_file(
        &host,
        "/src/A.vue",
        &vue_source("--brand"),
        FileLanguage::vue(),
    );
    let both = IndexDemand::of([
        IndexDemandPartition::CssVariables,
        IndexDemandPartition::StyleSelectors,
    ]);
    assert_ne!(both.scope(), css_demand().scope());

    let (variables, _) =
        host.workspace_index_candidates(&both, css_keys("--brand"), WIDE, None, &never());
    let (classes, work) =
        host.workspace_index_candidates(&both, selector_keys("a"), WIDE, None, &never());
    assert_eq!(variables.status, EnumerationStatus::Complete);
    assert_eq!(classes.status, EnumerationStatus::Complete);
    assert_eq!(classes.candidates.len(), 1);
    assert_eq!(
        work,
        IndexReconcileWork::default(),
        "the second read is warm"
    );

    // The narrower demand reuses the slot but proves its own population once.
    let (narrow, narrow_work) =
        host.workspace_index_candidates(&css_demand(), css_keys("--brand"), WIDE, None, &never());
    assert_eq!(narrow_work.projections, 0);
    assert_eq!(narrow_work.unchanged, 1);
    assert_eq!(fingerprint(&narrow), fingerprint(&variables));
}

#[test]
fn closing_the_host_releases_the_index() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_file(
        &host,
        "/src/A.vue",
        &vue_source("--brand"),
        FileLanguage::vue(),
    );
    let demand = css_demand();
    let (open, _) =
        host.workspace_index_candidates(&demand, css_keys("--brand"), WIDE, None, &never());
    assert!(!open.candidates.is_empty());
    drop(open);

    host.close();
    let store = host.project_type_store().workspace_index().snapshot();
    assert_eq!(store.contribution_count(), 0);
    assert_eq!(store.posting_count(), 0);
    let (closed, _) =
        host.workspace_index_candidates(&demand, css_keys("--brand"), WIDE, None, &never());
    assert!(closed.candidates.is_empty());
    assert_eq!(closed.status, EnumerationStatus::Complete);
}

#[test]
fn style_definitions_count_each_block_once_however_many_occurrences_it_holds() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_file(
        &host,
        "/src/A.vue",
        "<template><div /></template>
<style>
:root { --x: 1; }
.a { --x: 2; color: var(--x); background: var(--x); }
</style>
<style>
.b { --x: 3; }
</style>",
        FileLanguage::vue(),
    );
    let flow = host.css_var_flow("--x", None).flow;
    assert_eq!(
        flow.style_definitions.len(),
        2,
        "one path per defining block"
    );
    assert_eq!(flow.style_var_usages.len(), 1, "one path per using block");
}

#[test]
fn a_walk_that_never_settles_answers_privately_and_warms_nothing() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_file(
        &host,
        "/src/A.vue",
        &vue_source("--brand"),
        FileLanguage::vue(),
    );
    upsert_file(
        &host,
        "/src/B.vue",
        &vue_source("--brand"),
        FileLanguage::vue(),
    );
    let demand = css_demand();
    let edits = Cell::new(0_u32);
    let edit_every_step = || {
        edits.set(edits.get() + 1);
        let comment = format!("<!-- {} -->{}", edits.get(), vue_source("--brand"));
        upsert_file(&host, "/src/B.vue", &comment, FileLanguage::vue());
        false
    };

    let (snapshot, status, _) = host.workspace_index_reconcile(&demand, &edit_every_step);
    assert_eq!(
        status,
        ReconcileStatus::Uncertified(vec![PartitionShortfall {
            surface: IndexSurface::CssVariables,
            cause: PartialCause::Stale,
            unavailable_units: Vec::new(),
        }])
    );
    assert!(snapshot.is_return_only());
    let store = host.project_type_store().workspace_index().snapshot();
    assert!(
        store.coverage(&demand.scope()).is_none(),
        "no population was certified"
    );

    let (page, _) =
        host.workspace_index_candidates(&demand, css_keys("--brand"), WIDE, None, &edit_every_step);
    assert!(
        !page.candidates.is_empty(),
        "positive candidates still answer"
    );
    assert!(matches!(
        page.status,
        EnumerationStatus::Partial {
            cause: PartialCause::Stale,
            ..
        }
    ));

    let (settled, _) =
        host.workspace_index_candidates(&demand, css_keys("--brand"), WIDE, None, &never());
    assert_eq!(settled.status, EnumerationStatus::Complete);
}

fn is_partial_with(page: &CandidatePage, expected: &PartialCause) -> bool {
    matches!(&page.status, EnumerationStatus::Partial { cause, .. } if cause == expected)
}

#[test]
fn a_lookup_outside_the_demand_returns_no_row_and_is_never_complete() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let source = "<script setup>\nconst alpha = 1;\n</script>\n<template><div class=\"klass\" /></template>\n<style>\n.klass { --brand: blue; }\n</style>";
    upsert_file(&host, "/src/A.vue", source, FileLanguage::vue());
    let names = || Lookup::Keys(vec![PostingKey::exact(PartitionFamily::Bindings, "alpha")]);
    let bindings = IndexDemand::of([IndexDemandPartition::Bindings]);
    let outside = PartialCause::OutsideDemand;

    // Never populated: not Complete-empty.
    let (page, _) = host.workspace_index_candidates(&css_demand(), names(), WIDE, None, &never());
    assert!(page.candidates.is_empty());
    assert!(is_partial_with(&page, &outside));

    // A neighbouring family of the shared stylesheet table.
    let (page, _) = host.workspace_index_candidates(
        &css_demand(),
        selector_keys("klass"),
        WIDE,
        None,
        &never(),
    );
    assert!(page.candidates.is_empty());
    assert!(is_partial_with(&page, &outside));

    // An empty demand certifies nothing.
    let (page, _) = host.workspace_index_candidates(
        &IndexDemand::of::<[IndexSurface; 0]>([]),
        css_keys("--brand"),
        WIDE,
        None,
        &never(),
    );
    assert!(is_partial_with(&page, &outside));

    // Rows another demand left in the store never answer this one, including
    // after their source was edited or deleted.
    let (page, _) = host.workspace_index_candidates(&bindings, names(), WIDE, None, &never());
    assert_eq!(page.status, EnumerationStatus::Complete);
    assert_eq!(page.candidates.len(), 1);
    upsert_file(
        &host,
        "/src/A.vue",
        &source.replace("alpha", "beta"),
        FileLanguage::vue(),
    );
    let (page, _) = host.workspace_index_candidates(&css_demand(), names(), WIDE, None, &never());
    assert!(page.candidates.is_empty(), "the edited-away binding");
    assert!(is_partial_with(&page, &outside));
    let (page, _) = host.workspace_index_candidates(&bindings, names(), WIDE, None, &never());
    assert_eq!(page.status, EnumerationStatus::Complete);
    assert!(page.candidates.is_empty(), "its own demand re-reads it");

    let (page, _) = host.workspace_index_candidates(&bindings, names(), WIDE, None, &never());
    assert!(page.candidates.is_empty());
    host.remove("/src/A.vue");
    let (page, _) = host.workspace_index_candidates(
        &css_demand(),
        Lookup::Keys(vec![PostingKey::exact(PartitionFamily::Bindings, "beta")]),
        WIDE,
        None,
        &never(),
    );
    assert!(page.candidates.is_empty(), "the deleted file's row");
    assert!(is_partial_with(&page, &outside));
}

#[test]
fn a_resumed_page_of_a_lookup_reaching_outside_the_demand_is_never_complete() {
    let host = VerterHost::new_standalone(HostConfig::default());
    for id in ["/src/A.vue", "/src/B.vue"] {
        upsert_file(&host, id, &vue_source("--brand"), FileLanguage::vue());
    }
    let demand = css_demand();
    let mixed = || {
        Lookup::Keys(vec![
            PostingKey::exact(PartitionFamily::StyleSelectors, "klass"),
            PostingKey::exact(PartitionFamily::CssVariables, "--brand"),
        ])
    };
    let outside = PartialCause::OutsideDemand;
    let tight = QueryBudgets {
        postings: 1,
        records: 1,
    };

    let (whole, _) = host.workspace_index_candidates(&demand, mixed(), WIDE, None, &never());
    assert!(is_partial_with(&whole, &outside));

    let mut seen = Vec::new();
    let mut resume = None;
    let mut pages = 0;
    let last = loop {
        let (page, _) =
            host.workspace_index_candidates(&demand, mixed(), tight, resume.take(), &never());
        pages += 1;
        seen.extend(fingerprint(&page));
        match &page.status {
            EnumerationStatus::Partial {
                cause: PartialCause::BudgetExhausted,
                continuation: Some(next),
                ..
            } => resume = Some(next.clone()),
            _ => break page,
        }
    };
    assert!(pages > 1, "the lookup was paged");
    assert_eq!(seen, fingerprint(&whole), "every in-demand candidate once");
    assert!(
        is_partial_with(&last, &outside),
        "the final page keeps the whole lookup's cause, got {:?}",
        last.status
    );
}

#[test]
fn style_surfaces_are_unsupported_without_style_analysis_and_the_rest_produce_on_demand() {
    use verter_semantic::analysis::AnalysisScope;

    let source = "<script setup>\nimport Foo from './Foo.vue';\nconst alpha = 1;\n</script>\n<template><Foo class=\"klass\" :style=\"{ '--brand': 'x' }\" /></template>\n<style>\n.klass { --brand: blue; }\n</style>";
    let with_scope = |scope: AnalysisScope| {
        let host = VerterHost::new_standalone(HostConfig {
            analysis_scope: Some(scope),
            ..HostConfig::default()
        });
        upsert_file(&host, "/src/A.vue", source, FileLanguage::vue());
        host
    };
    let narrowed = [
        AnalysisScope::ESSENTIAL,
        AnalysisScope::IMPORTS,
        AnalysisScope::NONE,
    ];

    // Style facts come from the eager style analysis the scope configures:
    // without it the host has no producer, and says so without reading a file.
    for (surface, lookup) in [
        (IndexSurface::CssVariables, css_keys("--brand")),
        (IndexSurface::StyleDefinitions, selector_keys("klass")),
    ] {
        let demand = IndexDemand::of([surface]);
        let full = with_scope(AnalysisScope::LSP);
        let (page, _) =
            full.workspace_index_candidates(&demand, lookup.clone(), WIDE, None, &never());
        assert_eq!(page.status, EnumerationStatus::Complete, "{surface:?}");
        assert!(!page.candidates.is_empty(), "{surface:?}");

        for scope in narrowed {
            let host = with_scope(scope);
            let (page, work) =
                host.workspace_index_candidates(&demand, lookup.clone(), WIDE, None, &never());
            assert!(
                is_partial_with(&page, &PartialCause::UnsupportedCoverage),
                "{surface:?} under {scope:?}: {:?}",
                page.status
            );
            assert_eq!(work.projections, 0, "no file was read for {surface:?}");
            let store = host.project_type_store().workspace_index().snapshot();
            assert_eq!(store.contribution_count(), 0);
            assert!(store.coverage(&demand.scope()).is_none());
        }
    }

    // The whole style-selector partition still produces its markup
    // references on demand, and a stylesheet lookup under it is not certified
    // because the definitions facet it was demanded with cannot be.
    let host = with_scope(AnalysisScope::ESSENTIAL);
    let selectors = IndexDemand::of([IndexDemandPartition::StyleSelectors]);
    let (page, work) =
        host.workspace_index_candidates(&selectors, selector_keys("klass"), WIDE, None, &never());
    assert!(is_partial_with(&page, &PartialCause::UnsupportedCoverage));
    assert!(page.candidates.is_empty());
    assert_eq!(
        work.projections, 1,
        "the markup references of the one carrier"
    );
    let (page, _) =
        host.workspace_index_candidates(&selectors, markup_class_walk(), WIDE, None, &never());
    assert_eq!(
        markup_references(&page)
            .into_iter()
            .map(|(_, name, _)| name)
            .collect::<Vec<_>>(),
        ["klass"]
    );

    // Script and template facts are produced on demand: the scope is the
    // policy for eager work, not a proof of what a demand can have. A
    // template needs the carrier's analysis, which a scope that runs no
    // analysis at all does not keep; that one answers unavailable, naming
    // the unit, never certified empty.
    let component_keys =
        || Lookup::Keys(vec![PostingKey::exact(PartitionFamily::Components, "Foo")]);
    for (partition, lookup, produced_under) in [
        (
            IndexDemandPartition::Components,
            component_keys(),
            &[AnalysisScope::ESSENTIAL, AnalysisScope::IMPORTS][..],
        ),
        (
            IndexDemandPartition::Bindings,
            Lookup::Keys(vec![PostingKey::exact(PartitionFamily::Bindings, "alpha")]),
            &narrowed[..],
        ),
        (
            IndexDemandPartition::Imports,
            Lookup::Keys(vec![PostingKey::reverse(
                PartitionFamily::Imports,
                "./Foo.vue",
            )]),
            &narrowed[..],
        ),
    ] {
        let demand = IndexDemand::of([partition]);
        let full = with_scope(AnalysisScope::LSP);
        let (expected, _) =
            full.workspace_index_candidates(&demand, lookup.clone(), WIDE, None, &never());
        assert_eq!(
            expected.status,
            EnumerationStatus::Complete,
            "{partition:?}"
        );
        assert!(!expected.candidates.is_empty(), "{partition:?}");
        for &scope in produced_under {
            let host = with_scope(scope);
            let (page, _) =
                host.workspace_index_candidates(&demand, lookup.clone(), WIDE, None, &never());
            assert_eq!(
                page.status,
                EnumerationStatus::Complete,
                "{partition:?} under {scope:?}"
            );
            assert_eq!(
                fingerprint(&page),
                fingerprint(&expected),
                "{partition:?} under {scope:?}"
            );
        }
    }

    let none = with_scope(AnalysisScope::NONE);
    let components = IndexDemand::of([IndexDemandPartition::Components]);
    let (page, _) =
        none.workspace_index_candidates(&components, component_keys(), WIDE, None, &never());
    assert!(is_partial_with(&page, &PartialCause::UnavailableInput));
    let EnumerationStatus::Partial { covered, .. } = &page.status else {
        unreachable!()
    };
    assert_eq!(covered.unavailable_units, [Arc::<str>::from("/src/A.vue")]);
    // The inventory facet needs no template and stays certified; the uses
    // facet is never certified empty.
    assert!(none
        .project_type_store()
        .workspace_index()
        .snapshot()
        .coverage(&components.scope())
        .is_none_or(|record| !record
            .partitions
            .contains(&IndexSurface::ComponentUses.id())));

    // What the host does produce stays certified under the same demand.
    let host = with_scope(AnalysisScope::ESSENTIAL);
    let both = IndexDemand::of([
        IndexDemandPartition::Bindings,
        IndexDemandPartition::CssVariables,
    ]);
    let names = Lookup::Keys(vec![PostingKey::exact(PartitionFamily::Bindings, "alpha")]);
    let (page, _) = host.workspace_index_candidates(&both, names, WIDE, None, &never());
    assert_eq!(page.status, EnumerationStatus::Complete);
    assert_eq!(page.candidates.len(), 1);
    let (page, work) =
        host.workspace_index_candidates(&both, css_keys("--brand"), WIDE, None, &never());
    assert!(is_partial_with(&page, &PartialCause::UnsupportedCoverage));
    assert_eq!(
        work,
        IndexReconcileWork::default(),
        "the supported half is warm"
    );
}

#[test]
fn css_var_flow_reports_at_the_public_boundary_that_a_partial_answer_is_partial() {
    let essential = VerterHost::new_standalone(HostConfig {
        analysis_level: AnalysisLevel::Essential,
        ..HostConfig::default()
    });
    upsert_file(
        &essential,
        "/src/A.vue",
        &vue_source("--brand"),
        FileLanguage::vue(),
    );
    let flow = essential.css_var_flow("--brand", None);
    assert!(is_partial_with(
        &flow.page,
        &PartialCause::UnsupportedCoverage
    ));
    assert!(flow.flow.style_definitions.is_empty());

    let full = VerterHost::new_standalone(HostConfig::default());
    upsert_file(
        &full,
        "/src/A.vue",
        &vue_source("--brand"),
        FileLanguage::vue(),
    );
    let certified = full.css_var_flow("--absent", None);
    assert_eq!(certified.page.status, EnumerationStatus::Complete);
    assert!(certified.flow.style_definitions.is_empty());
}

#[test]
fn a_failed_load_is_not_part_of_the_population_and_keeps_the_warm_path() {
    let host = VerterHost::new_standalone(HostConfig::default());
    for id in ["/src/A.vue", "/src/B.vue"] {
        upsert_file(&host, id, &vue_source("--brand"), FileLanguage::vue());
    }
    let demand = css_demand();
    let (first, _) =
        host.workspace_index_candidates(&demand, css_keys("--brand"), WIDE, None, &never());
    assert_eq!(first.status, EnumerationStatus::Complete);

    assert!(!host.ensure_loaded("/src/Nope.vue"));
    upsert_file(
        &host,
        "/src/B.vue",
        &vue_source("--other"),
        FileLanguage::vue(),
    );
    let (edited, work) =
        host.workspace_index_candidates(&demand, css_keys("--brand"), WIDE, None, &never());
    assert_eq!(edited.status, EnumerationStatus::Complete);
    assert_eq!(
        work.population_checks, 1,
        "a failed load publishes no source transition: only the edited file is re-read"
    );
    assert_eq!(work.projections, 1);

    let (warm, warm_work) =
        host.workspace_index_candidates(&demand, css_keys("--brand"), WIDE, None, &never());
    assert_eq!(warm.status, EnumerationStatus::Complete);
    assert_eq!(warm_work, IndexReconcileWork::default());
}

#[test]
fn the_host_route_resumes_its_own_continuation_and_refuses_a_foreign_one() {
    let host = VerterHost::new_standalone(HostConfig::default());
    for id in ["/src/A.vue", "/src/B.vue"] {
        upsert_file(&host, id, &vue_source("--brand"), FileLanguage::vue());
    }
    let demand = css_demand();
    let tight = QueryBudgets {
        postings: 1,
        records: 1,
    };
    let (whole, _) =
        host.workspace_index_candidates(&demand, css_keys("--brand"), WIDE, None, &never());

    let mut seen = Vec::new();
    let mut resume = None;
    loop {
        let (page, _) = host.workspace_index_candidates(
            &demand,
            css_keys("--brand"),
            tight,
            resume.take(),
            &never(),
        );
        seen.extend(fingerprint(&page));
        match page.status {
            EnumerationStatus::Partial {
                cause: PartialCause::BudgetExhausted,
                continuation: Some(next),
                ..
            } => resume = Some(next),
            EnumerationStatus::Complete => break,
            other => panic!("unexpected status {other:?}"),
        }
    }
    assert_eq!(seen, fingerprint(&whole), "every candidate once, in order");

    let first_page = |host: &VerterHost| {
        let (page, _) =
            host.workspace_index_candidates(&demand, css_keys("--brand"), tight, None, &never());
        match page.status {
            EnumerationStatus::Partial {
                continuation: Some(next),
                ..
            } => next,
            other => panic!("expected a continuation, got {other:?}"),
        }
    };

    // A different lookup: not an empty success.
    let (foreign, _) = host.workspace_index_candidates(
        &demand,
        css_keys("--other"),
        WIDE,
        Some(first_page(&host)),
        &never(),
    );
    assert!(foreign.candidates.is_empty());
    assert!(is_partial_with(&foreign, &PartialCause::Stale));

    // Another project's index at the same generation.
    let elsewhere = VerterHost::new_standalone(HostConfig::default());
    for id in ["/src/A.vue", "/src/B.vue"] {
        upsert_file(&elsewhere, id, &vue_source("--brand"), FileLanguage::vue());
    }
    let (other_store, _) = elsewhere.workspace_index_candidates(
        &demand,
        css_keys("--brand"),
        WIDE,
        Some(first_page(&host)),
        &never(),
    );
    assert!(is_partial_with(&other_store, &PartialCause::Stale));

    // The index moved on after the page was issued.
    let stale = first_page(&host);
    upsert_file(
        &host,
        "/src/B.vue",
        &vue_source("--other"),
        FileLanguage::vue(),
    );
    let (moved_on, _) =
        host.workspace_index_candidates(&demand, css_keys("--brand"), WIDE, Some(stale), &never());
    assert!(is_partial_with(&moved_on, &PartialCause::Stale));
}

#[test]
fn a_request_local_answer_issues_no_continuation_it_could_never_honor() {
    let host = VerterHost::new_standalone(HostConfig::default());
    for id in ["/src/A.vue", "/src/B.vue"] {
        upsert_file(&host, id, &vue_source("--brand"), FileLanguage::vue());
    }
    let edits = Cell::new(0_u32);
    let edit_every_step = || {
        edits.set(edits.get() + 1);
        let comment = format!("<!-- {} -->{}", edits.get(), vue_source("--brand"));
        upsert_file(&host, "/src/B.vue", &comment, FileLanguage::vue());
        false
    };
    let (page, _) = host.workspace_index_candidates(
        &css_demand(),
        css_keys("--brand"),
        QueryBudgets {
            postings: 1,
            records: 1,
        },
        None,
        &edit_every_step,
    );
    assert!(matches!(
        page.status,
        EnumerationStatus::Partial {
            continuation: None,
            ..
        }
    ));
}

/// The production LSP host's scope: template-free. Every template-backed
/// surface must still certify from cold, unopened files.
fn build_host() -> VerterHost {
    VerterHost::new_standalone(HostConfig {
        analysis_scope: Some(verter_semantic::analysis::AnalysisScope::BUILD),
        ..HostConfig::default()
    })
}

const COLD_VUE: &str = "<script setup>\nconst on = true;\nconst el = document.body;\nel.style.setProperty('--brand', 'red');\n</script>\n<template><Child class=\"card wide\" :class=\"{ 'active': on }\" :style=\"{ '--brand': 'blue' }\" /></template>\n<style>\n.card { --brand: blue; color: var(--brand); }\n</style>";

const COLD_SVELTE: &str = "<script>\nlet a = 1;\n</script>\n<div class=\"card\" style=\"--brand: red; color: blue\" style:--accent={a}><Badge --tone=\"dark\" /></div>\n<style>\n.card { color: var(--brand); }\n</style>";

/// Every markup class reference, keyed by class name.
fn markup_class_walk() -> Lookup {
    Lookup::Prefix {
        partition: PartitionFamily::StyleSelectors,
        family: KeyFamily::Reverse,
        version: 0,
        prefix: Arc::from(""),
    }
}

fn component_keys(tag: &str) -> Lookup {
    Lookup::Keys(vec![PostingKey::exact(PartitionFamily::Components, tag)])
}

fn sorted_paths(list: &[Arc<std::path::Path>]) -> Vec<String> {
    let mut paths: Vec<String> = list.iter().map(|p| p.display().to_string()).collect();
    paths.sort();
    paths
}

/// The markup class references of an answer: locator, name and quality.
fn markup_references(page: &CandidatePage) -> Vec<(String, String, FactQuality)> {
    let mut tokens: Vec<(String, String, FactQuality)> = page
        .candidates
        .iter()
        .filter_map(|candidate| {
            let record = candidate.record()?;
            match &record.row {
                IndexRow::Stylesheet(row)
                    if row.role == StyleRole::Reference
                        && row.surface == StyleSurface::Template =>
                {
                    Some((
                        candidate.contribution.basis.locator.to_string(),
                        row.name.to_string(),
                        record.quality,
                    ))
                }
                _ => None,
            }
        })
        .collect();
    tokens.sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));
    tokens
}

#[test]
fn a_build_scope_host_certifies_template_backed_surfaces_from_cold_files() {
    let host = build_host();
    upsert_file(&host, "/src/A.vue", COLD_VUE, FileLanguage::vue());
    upsert_file(&host, "/src/B.svelte", COLD_SVELTE, FileLanguage::svelte());

    // Component uses: the lazily produced template is the value certified,
    // so its rows are there.
    let components = IndexDemand::of([IndexDemandPartition::Components]);
    let (uses, work) =
        host.workspace_index_candidates(&components, component_keys("Child"), WIDE, None, &never());
    assert_eq!(uses.status, EnumerationStatus::Complete);
    assert_eq!(
        uses.candidates.len(),
        1,
        "the cold template's component use"
    );
    assert_eq!(
        work.projections, 4,
        "two surfaces of two carriers, once each"
    );
    let (badge, _) =
        host.workspace_index_candidates(&components, component_keys("Badge"), WIDE, None, &never());
    assert_eq!(badge.status, EnumerationStatus::Complete);
    assert_eq!(
        badge.candidates.len(),
        1,
        "svelte uses come from its template facts"
    );

    // Style selectors: definitions by their exact key, markup references by
    // the reverse walk, both frameworks.
    let selectors = IndexDemand::of([IndexDemandPartition::StyleSelectors]);
    let (definitions, _) =
        host.workspace_index_candidates(&selectors, selector_keys("card"), WIDE, None, &never());
    assert_eq!(definitions.status, EnumerationStatus::Complete);
    assert_eq!(definitions.candidates.len(), 2);
    let (references, _) =
        host.workspace_index_candidates(&selectors, markup_class_walk(), WIDE, None, &never());
    assert_eq!(references.status, EnumerationStatus::Complete);
    let reference =
        |locator: &str, name: &str, quality| (locator.to_string(), name.to_string(), quality);
    assert_eq!(
        markup_references(&references),
        [
            reference("/src/A.vue", "active", FactQuality::Approximate),
            reference("/src/A.vue", "card", FactQuality::Exact),
            reference("/src/A.vue", "wide", FactQuality::Exact),
            reference("/src/B.svelte", "card", FactQuality::Exact),
        ]
    );

    // CSS variables: every leg, including svelte inline styles.
    let flow = host.css_var_flow("--brand", None);
    assert_eq!(flow.page.status, EnumerationStatus::Complete);
    assert_eq!(sorted_paths(&flow.flow.style_definitions), ["/src/A.vue"]);
    assert_eq!(
        sorted_paths(&flow.flow.style_var_usages),
        ["/src/A.vue", "/src/B.svelte"]
    );
    assert_eq!(
        sorted_paths(&flow.flow.template_definitions),
        ["/src/A.vue", "/src/B.svelte"]
    );
    assert_eq!(
        sorted_paths(&flow.flow.script_manipulations),
        ["/src/A.vue"]
    );

    // Repeated warm queries read nothing.
    for demand in [&components, &selectors] {
        let (_, _, warm) = host.workspace_index_reconcile(demand, &never());
        assert_eq!(warm, IndexReconcileWork::default(), "{demand:?}");
    }
}

#[test]
fn facts_the_ide_already_produced_answer_exactly_as_cold_production_does() {
    let cold = build_host();
    upsert_file(&cold, "/src/A.vue", COLD_VUE, FileLanguage::vue());
    upsert_file(&cold, "/src/B.svelte", COLD_SVELTE, FileLanguage::svelte());

    // A full-scope host holds the template on its analysis snapshot already,
    // as an editor that compiled the file does.
    let warmed = VerterHost::new_standalone(HostConfig::default());
    upsert_file(&warmed, "/src/A.vue", COLD_VUE, FileLanguage::vue());
    upsert_file(
        &warmed,
        "/src/B.svelte",
        COLD_SVELTE,
        FileLanguage::svelte(),
    );
    assert!(warmed
        .get_analysis("/src/A.vue")
        .is_some_and(|analysis| analysis.template.is_some()));

    let demand = IndexDemand::of([
        IndexDemandPartition::Components,
        IndexDemandPartition::StyleSelectors,
        IndexDemandPartition::CssVariables,
    ]);
    for lookup in [
        component_keys("Child"),
        component_keys("Badge"),
        selector_keys("card"),
        markup_class_walk(),
        css_keys("--brand"),
    ] {
        let (from_cold, cold_work) =
            cold.workspace_index_candidates(&demand, lookup.clone(), WIDE, None, &never());
        let (from_warmed, warmed_work) =
            warmed.workspace_index_candidates(&demand, lookup.clone(), WIDE, None, &never());
        assert_eq!(from_cold.status, EnumerationStatus::Complete, "{lookup:?}");
        assert_eq!(
            fingerprint(&from_cold),
            fingerprint(&from_warmed),
            "{lookup:?}"
        );
        assert_eq!(cold_work, warmed_work, "{lookup:?}");
    }
}

#[test]
fn surfaces_certify_separately_when_a_template_is_out_of_reach() {
    let host = build_host();
    upsert_file(
        &host,
        "/src/A.vue",
        "<script setup>\nconst a = 1;\n</script>\n<template src=\"./a.html\"></template>\n<style>\n.card { --brand: red; }\n</style>",
        FileLanguage::vue(),
    );

    // The inventory needs no template.
    let inventory = IndexDemand::of([IndexSurface::ComponentInventory]);
    let (snapshot, status, work) = host.workspace_index_reconcile(&inventory, &never());
    assert_eq!(status, ReconcileStatus::Current);
    assert_eq!(
        snapshot
            .contributions_in_partition(&IndexSurface::ComponentInventory.id())
            .count(),
        1
    );
    assert_eq!(work.projections, 1);

    // Uses need the template an external source keeps out of reach: the unit
    // is named, and nothing is certified empty.
    let uses = IndexDemand::of([IndexSurface::ComponentUses]);
    let (snapshot, status, _) = host.workspace_index_reconcile(&uses, &never());
    let ReconcileStatus::Uncertified(shortfalls) = status else {
        panic!("an unproduced template must not certify: {status:?}");
    };
    assert_eq!(shortfalls.len(), 1);
    assert_eq!(shortfalls[0].surface, IndexSurface::ComponentUses);
    assert_eq!(shortfalls[0].cause, PartialCause::UnavailableInput);
    assert_eq!(
        shortfalls[0].unavailable_units,
        [Arc::<str>::from("/src/A.vue")]
    );
    assert!(snapshot.coverage(&uses.scope()).is_none());

    // Under the whole partition a definition lookup is not certified, because
    // the markup references it was demanded with are not; the rows still answer.
    let selectors = IndexDemand::of([IndexDemandPartition::StyleSelectors]);
    let (page, _) =
        host.workspace_index_candidates(&selectors, selector_keys("card"), WIDE, None, &never());
    assert!(is_partial_with(&page, &PartialCause::UnavailableInput));
    assert_eq!(page.candidates.len(), 1);
    let EnumerationStatus::Partial { covered, .. } = &page.status else {
        unreachable!()
    };
    assert_eq!(covered.unavailable_units, [Arc::<str>::from("/src/A.vue")]);

    // Demanded on its own, the definition surface certifies.
    let definitions = IndexDemand::of([IndexSurface::StyleDefinitions]);
    let (page, _) =
        host.workspace_index_candidates(&definitions, selector_keys("card"), WIDE, None, &never());
    assert_eq!(page.status, EnumerationStatus::Complete);
    assert_eq!(page.candidates.len(), 1);
}

#[test]
fn an_edit_reads_one_file_and_a_removal_or_eviction_retires_every_surface_of_it() {
    let host = VerterHost::new_standalone(HostConfig::default());
    for id in ["/src/A.vue", "/src/B.vue", "/src/C.vue"] {
        upsert_file(&host, id, &vue_source("--brand"), FileLanguage::vue());
    }
    let all = IndexDemand::of(IndexSurface::ALL);
    let (_, status, cold) = host.workspace_index_reconcile(&all, &never());
    assert_eq!(status, ReconcileStatus::Current);
    assert_eq!(cold.population_checks, 3, "the one whole read");
    assert_eq!(cold.projections, 24, "eight surfaces of three carriers");

    upsert_file(
        &host,
        "/src/B.vue",
        &vue_source("--other"),
        FileLanguage::vue(),
    );
    let (_, status, edited) = host.workspace_index_reconcile(&all, &never());
    assert_eq!(status, ReconcileStatus::Current);
    assert_eq!(
        edited,
        IndexReconcileWork {
            population_checks: 1,
            projections: 8,
            unchanged: 0,
            retired: 0,
            publish_attempts: 1,
        },
        "one file re-read, its eight slots re-projected, no other slot visited"
    );
    let (_, _, warm) = host.workspace_index_reconcile(&all, &never());
    assert_eq!(warm, IndexReconcileWork::default());

    host.remove("/src/C.vue");
    let (snapshot, status, removed) = host.workspace_index_reconcile(&all, &never());
    assert_eq!(status, ReconcileStatus::Current);
    assert_eq!(removed.population_checks, 1);
    assert_eq!(removed.projections, 0);
    assert_eq!(
        removed.retired, 8,
        "every surface's slot of the removed file"
    );
    assert_eq!(snapshot.contribution_count(), 16);

    // An eviction is the host's own membership change: the scheduler keeps
    // the node, the index still learns of it from one file.
    host.evict("/src/A.vue");
    let (snapshot, status, evicted) = host.workspace_index_reconcile(&all, &never());
    assert_eq!(status, ReconcileStatus::Current);
    assert_eq!(evicted.population_checks, 1);
    assert_eq!(evicted.retired, 8);
    assert_eq!(snapshot.contribution_count(), 8);
    let (_, _, warm) = host.workspace_index_reconcile(&all, &never());
    assert_eq!(warm, IndexReconcileWork::default());
}

#[test]
fn svelte_inline_style_custom_properties_are_template_definitions_with_exact_spans() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_file(&host, "/src/B.svelte", COLD_SVELTE, FileLanguage::svelte());
    for name in ["--brand", "--accent", "--tone"] {
        let answer = host.css_var_candidates(name, WIDE, None, &never());
        assert_eq!(answer.page.status, EnumerationStatus::Complete, "{name}");
        assert_eq!(
            sorted_paths(&answer.flow.template_definitions),
            ["/src/B.svelte"],
            "{name}"
        );
        let template_origins: Vec<&RecordOrigin> = answer
            .page
            .candidates
            .iter()
            .filter_map(|candidate| {
                let record = candidate.record()?;
                match &record.row {
                    IndexRow::Stylesheet(row) if row.surface == StyleSurface::Template => {
                        Some(&record.origins[0])
                    }
                    _ => None,
                }
            })
            .collect();
        assert_eq!(template_origins.len(), 1, "{name}");
        let RecordOrigin::Authored(anchor) = template_origins[0] else {
            panic!("{name}: svelte markup records the authored name: {template_origins:?}");
        };
        let start = usize::try_from(anchor.range.start().get()).unwrap();
        let end = usize::try_from(anchor.range.end().get()).unwrap();
        assert_eq!(&COLD_SVELTE[start..end], name);
    }
}

#[test]
fn template_facts_answer_a_cold_carrier_under_a_template_free_scope_with_coverage() {
    let host = build_host();
    upsert_file(
        &host,
        "/src/A.vue",
        "<script setup>\nimport Child from './Child.vue';\nconst t = 'x';\n</script>\n<template><Child :title=\"t\" class=\"card\">b</Child></template>",
        FileLanguage::vue(),
    );
    upsert_file(
        &host,
        "/src/External.vue",
        "<script setup>\nconst a = 1;\n</script>\n<template src=\"./a.html\"></template>",
        FileLanguage::vue(),
    );
    upsert_file(
        &host,
        "/src/util.ts",
        "export const one = 1;\n",
        FileLanguage::script_ts(),
    );
    assert!(
        host.get_analysis("/src/A.vue")
            .is_some_and(|analysis| analysis.template.is_none()),
        "the template-free scope holds no template for a cold carrier"
    );

    let cold = host.template_facts("/src/A.vue");
    assert_eq!(cold.coverage, FactCoverage::Complete);
    let template = cold.value.expect("a produced template");
    assert_eq!(
        template
            .components
            .iter()
            .map(|usage| usage.name.as_str())
            .collect::<Vec<_>>(),
        ["Child"]
    );
    let usage = &template.components[0];
    assert_eq!(usage.import_source.as_deref(), Some("./Child.vue"));
    // Every authored attribute of a component use is a recorded prop: the
    // bound `:title` and the static `class`, nothing else.
    let mut props: Vec<(&str, bool)> = usage
        .props
        .iter()
        .map(|prop| (prop.name.as_str(), prop.is_bound))
        .collect();
    props.sort_unstable();
    assert_eq!(props, [("class", false), ("title", true)]);

    // The same value answers again: the production is retained.
    let again = host.template_facts("/src/A.vue");
    assert_eq!(again.coverage, FactCoverage::Complete);
    assert_eq!(
        again.value.map(|t| t.components.len()),
        Some(template.components.len())
    );

    let external = host.template_facts("/src/External.vue");
    assert!(
        matches!(external.coverage, FactCoverage::Unavailable { .. }),
        "{:?}",
        external.coverage
    );
    assert!(external.value.is_none());

    assert_eq!(
        host.template_facts("/src/util.ts").coverage,
        FactCoverage::Absent
    );
    assert!(matches!(
        host.template_facts("/src/Missing.vue").coverage,
        FactCoverage::Unavailable { .. }
    ));
}

#[test]
fn scopes_reconciled_in_turn_each_see_an_edit_an_addition_and_a_removal() {
    let host = VerterHost::new_standalone(HostConfig::default());
    for id in ["/src/A.vue", "/src/B.vue"] {
        upsert_file(&host, id, &vue_source("--brand"), FileLanguage::vue());
    }
    let definitions = IndexDemand::of([IndexSurface::StyleDefinitions]);
    let imports = IndexDemand::of([IndexSurface::Imports]);
    let variables = IndexDemand::of([IndexSurface::CssVariables]);
    let scopes = [&definitions, &imports, &variables];
    for demand in scopes {
        let (_, status, _) = host.workspace_index_reconcile(demand, &never());
        assert_eq!(status, ReconcileStatus::Current, "{demand:?}");
    }
    let definers = |host: &VerterHost, class: &str| {
        let (page, _) = host.workspace_index_candidates(
            &definitions,
            selector_keys(class),
            WIDE,
            None,
            &never(),
        );
        assert_eq!(page.status, EnumerationStatus::Complete, "{class}");
        let mut locators: Vec<String> = page
            .candidates
            .iter()
            .map(|candidate| candidate.contribution.basis.locator.to_string())
            .collect();
        locators.sort();
        locators
    };
    let tracked = |host: &VerterHost| {
        let (page, work) =
            host.workspace_index_units(&imports, IndexSurface::Imports, WIDE, &never());
        assert_eq!(page.status, EnumerationStatus::Complete);
        (unit_locators(&page), work)
    };

    // An edit, seen first by the scope that consumes the journal, then by the
    // scopes that follow: each re-reads the one file's slot of its surface.
    upsert_file(
        &host,
        "/src/A.vue",
        &vue_source("--brand").replace(".a {", ".card {"),
        FileLanguage::vue(),
    );
    assert_eq!(definers(&host, "card"), ["/src/A.vue"]);
    assert_eq!(definers(&host, "a"), ["/src/B.vue"]);
    let (_, status, later) = host.workspace_index_reconcile(&imports, &never());
    assert_eq!(status, ReconcileStatus::Current);
    assert_eq!(
        later,
        IndexReconcileWork {
            population_checks: 0,
            projections: 1,
            unchanged: 0,
            retired: 0,
            publish_attempts: 1,
        },
        "the journal was consumed by the first scope; the debt remains per surface"
    );
    let (page, _) =
        host.workspace_index_candidates(&variables, css_keys("--brand"), WIDE, None, &never());
    assert_eq!(page.status, EnumerationStatus::Complete);
    assert_eq!(
        page.candidates
            .iter()
            .filter(|candidate| candidate.contribution.basis.locator.as_ref() == "/src/A.vue")
            .count(),
        4,
        "the edited file's variable rows come from its new revision"
    );

    // An addition, seen first by the units walk, then by the other scopes.
    upsert_file(
        &host,
        "/src/C.ts",
        "export const one = 1;\n",
        FileLanguage::script_ts(),
    );
    let (units, _) = tracked(&host);
    assert_eq!(units, ["/src/A.vue", "/src/B.vue", "/src/C.ts"]);
    let (_, status, work) = host.workspace_index_reconcile(&definitions, &never());
    assert_eq!(status, ReconcileStatus::Current);
    assert_eq!(work.projections, 1, "the new file's definitions slot");
    let (_, status, work) = host.workspace_index_reconcile(&variables, &never());
    assert_eq!(status, ReconcileStatus::Current);
    assert_eq!(work.projections, 1, "the new file's variables slot");

    // A removal, seen first by the variables scope, then by the others.
    host.remove("/src/B.vue");
    let (_, status, work) = host.workspace_index_reconcile(&variables, &never());
    assert_eq!(status, ReconcileStatus::Current);
    assert_eq!(work.retired, 3, "every surface's slot of the removed file");
    assert_eq!(definers(&host, "a"), Vec::<String>::new());
    let (units, work) = tracked(&host);
    assert_eq!(units, ["/src/A.vue", "/src/C.ts"]);
    assert_eq!(work.projections, 0);

    // Every scope now answers exactly as a fresh host does.
    let fresh = VerterHost::new_standalone(HostConfig::default());
    upsert_file(
        &fresh,
        "/src/A.vue",
        &vue_source("--brand").replace(".a {", ".card {"),
        FileLanguage::vue(),
    );
    upsert_file(
        &fresh,
        "/src/C.ts",
        "export const one = 1;\n",
        FileLanguage::script_ts(),
    );
    for (demand, lookup) in [
        (&definitions, selector_keys("card")),
        (&variables, css_keys("--brand")),
    ] {
        let (live, _) =
            host.workspace_index_candidates(demand, lookup.clone(), WIDE, None, &never());
        let (clean, _) = fresh.workspace_index_candidates(demand, lookup, WIDE, None, &never());
        assert_eq!(live.status, EnumerationStatus::Complete);
        assert_eq!(fingerprint(&live), fingerprint(&clean));
    }
    assert_eq!(tracked(&host).0, tracked(&fresh).0);
}

#[test]
fn a_removal_consumed_by_a_cancelled_walk_is_still_retired_by_the_next() {
    let host = VerterHost::new_standalone(HostConfig::default());
    for id in ["/src/A.vue", "/src/B.vue"] {
        upsert_file(&host, id, &vue_source("--brand"), FileLanguage::vue());
    }
    let demand = css_demand();
    let (_, status, _) = host.workspace_index_reconcile(&demand, &never());
    assert_eq!(status, ReconcileStatus::Current);

    // The walk that reads the removal from the journal is cancelled before
    // its delta commits: the removal is consumed, the retirement is owed.
    host.remove("/src/A.vue");
    let (_, status, _) = host.workspace_index_reconcile(&demand, &|| true);
    assert_eq!(status, ReconcileStatus::Cancelled);
    let store = host.project_type_store().workspace_index().snapshot();
    assert_eq!(store.contribution_count(), 2, "nothing was committed");

    let (page, work) =
        host.workspace_index_candidates(&demand, css_keys("--brand"), WIDE, None, &never());
    assert_eq!(page.status, EnumerationStatus::Complete);
    assert!(page
        .candidates
        .iter()
        .all(|candidate| candidate.contribution.basis.locator.as_ref() == "/src/B.vue"));
    assert_eq!(
        work.retired, 1,
        "the owed retirement is paid by the next commit"
    );
    assert_eq!(
        work.population_checks, 0,
        "the journal was already consumed"
    );
    let store = host.project_type_store().workspace_index().snapshot();
    assert_eq!(store.contribution_count(), 1);

    // Another surface demanded later owes nothing for the removed file.
    let (units, work) = host.workspace_index_units(
        &IndexDemand::of([IndexSurface::Imports]),
        IndexSurface::Imports,
        WIDE,
        &never(),
    );
    assert_eq!(units.status, EnumerationStatus::Complete);
    assert_eq!(unit_locators(&units), ["/src/B.vue"]);
    assert_eq!(work.retired, 0);
}

fn unit_locators(page: &UnitPage) -> Vec<String> {
    let mut units: Vec<String> = page
        .units
        .iter()
        .map(|slot| slot.basis.locator.to_string())
        .collect();
    units.sort();
    units
}

fn units_partial_with(page: &UnitPage, expected: &PartialCause) -> bool {
    matches!(&page.status, EnumerationStatus::Partial { cause, .. } if cause == expected)
}

#[test]
fn a_surface_population_is_counted_certified_and_partial_past_its_budget() {
    let host = build_host();
    for id in ["/src/A.vue", "/src/B.vue"] {
        upsert_file(&host, id, &vue_source("--brand"), FileLanguage::vue());
    }
    upsert_file(
        &host,
        "/src/util.ts",
        "export const one = 1;\n",
        FileLanguage::script_ts(),
    );

    // The component inventory reaches the carriers without a template walk.
    let inventory = IndexDemand::of([IndexSurface::ComponentInventory]);
    let (page, work) =
        host.workspace_index_units(&inventory, IndexSurface::ComponentInventory, WIDE, &never());
    assert_eq!(page.status, EnumerationStatus::Complete);
    assert_eq!(unit_locators(&page), ["/src/A.vue", "/src/B.vue"]);
    assert_eq!(page.work.posting_visits, 2, "one visit per slot");
    assert_eq!(work.projections, 2);

    // Imports reach every tracked file, plain scripts included.
    let imports = IndexDemand::of([IndexSurface::Imports]);
    let (all, _) = host.workspace_index_units(&imports, IndexSurface::Imports, WIDE, &never());
    assert_eq!(all.status, EnumerationStatus::Complete);
    assert_eq!(
        unit_locators(&all),
        ["/src/A.vue", "/src/B.vue", "/src/util.ts"]
    );
    let (_, warm) = host.workspace_index_units(&imports, IndexSurface::Imports, WIDE, &never());
    assert_eq!(
        warm,
        IndexReconcileWork::default(),
        "a repeated walk reads no file"
    );

    let tight = QueryBudgets {
        postings: 1,
        records: 1,
    };
    let (cut, _) = host.workspace_index_units(
        &inventory,
        IndexSurface::ComponentInventory,
        tight,
        &never(),
    );
    assert!(units_partial_with(&cut, &PartialCause::BudgetExhausted));
    assert_eq!(cut.units.len(), 1);
    assert_eq!(cut.work.posting_visits, 1);

    // A surface outside the demand answers nothing and is never complete.
    let (outside, _) =
        host.workspace_index_units(&inventory, IndexSurface::Imports, WIDE, &never());
    assert!(outside.units.is_empty());
    assert!(units_partial_with(&outside, &PartialCause::OutsideDemand));

    // A removed file leaves the population from one re-read.
    host.remove("/src/B.vue");
    let (after, work) = host.workspace_index_units(&imports, IndexSurface::Imports, WIDE, &never());
    assert_eq!(after.status, EnumerationStatus::Complete);
    assert_eq!(unit_locators(&after), ["/src/A.vue", "/src/util.ts"]);
    assert_eq!(work.population_checks, 1);
}

#[test]
fn a_surface_the_host_cannot_produce_has_no_population_and_reads_no_file() {
    let host = VerterHost::new_standalone(HostConfig {
        analysis_scope: Some(verter_semantic::analysis::AnalysisScope::ESSENTIAL),
        ..HostConfig::default()
    });
    upsert_file(
        &host,
        "/src/A.vue",
        &vue_source("--brand"),
        FileLanguage::vue(),
    );
    let demand = IndexDemand::of([IndexSurface::StyleDefinitions]);
    let (page, work) =
        host.workspace_index_units(&demand, IndexSurface::StyleDefinitions, WIDE, &never());
    assert!(page.units.is_empty());
    assert!(units_partial_with(
        &page,
        &PartialCause::UnsupportedCoverage
    ));
    assert_eq!(work.projections, 0);
}

const REGISTRATION_SOURCE: &str = "<script setup lang=\"ts\">\nimport { provide, inject } from 'vue'\nimport { useCartStore } from '@/stores/cart'\ndefineEmits(['save'])\nprovide('theme', 'dark')\nconst user = inject('user')\nconst cart = useCartStore()\n</script>\n<template><Child @close=\"user\" /></template>";

fn registration_rows(page: &CandidatePage) -> Vec<(String, String, bool, String)> {
    let mut rows: Vec<(String, String, bool, String)> = page
        .candidates
        .iter()
        .filter_map(|candidate| match &candidate.record()?.row {
            IndexRow::Registration(row) => {
                let key = match &row.key {
                    RegistrationKey::String(text) | RegistrationKey::Symbol(text) => text,
                };
                Some((
                    candidate.contribution.basis.locator.to_string(),
                    format!("{}:{key}", row.domain),
                    row.is_usage,
                    row.operation.to_string(),
                ))
            }
            _ => None,
        })
        .collect();
    rows.sort();
    rows
}

#[test]
fn registrations_answer_by_domain_key_and_keep_declarations_apart_from_uses() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_file(
        &host,
        "/src/A.vue",
        REGISTRATION_SOURCE,
        FileLanguage::vue(),
    );
    upsert_file(
        &host,
        "/src/B.vue",
        "<script setup>\nconst a = 1;\n</script>\n<template><div>{{ a }}</div></template>",
        FileLanguage::vue(),
    );
    let demand = IndexDemand::of([IndexSurface::Registrations]);
    let keyed = |domain: &str, key: &str| {
        let lookup = Lookup::Keys(vec![registration_posting_key(domain, key)]);
        let (page, _) = host.workspace_index_candidates(&demand, lookup, WIDE, None, &never());
        assert_eq!(page.status, EnumerationStatus::Complete, "{domain}:{key}");
        registration_rows(&page)
    };
    let row = |key: &str, is_usage: bool, operation: &str| {
        (
            "/src/A.vue".to_string(),
            key.to_string(),
            is_usage,
            operation.to_string(),
        )
    };
    assert_eq!(
        keyed(registration_domain::PROVIDE_INJECT, "theme"),
        [row("provide-inject:theme", false, "provide")]
    );
    assert_eq!(
        keyed(registration_domain::PROVIDE_INJECT, "user"),
        [row("provide-inject:user", true, "inject")]
    );
    assert_eq!(
        keyed(registration_domain::STORE, "useCartStore"),
        [row("store:useCartStore", true, "use")]
    );
    assert_eq!(
        keyed(registration_domain::EVENT, "save"),
        [row("event:save", false, "emit")]
    );
    assert_eq!(
        keyed(registration_domain::EVENT, "close"),
        [row("event:close", true, "listen")]
    );
    // The same spelling in another domain is another key.
    assert!(keyed(registration_domain::EVENT, "theme").is_empty());

    // A domain walk answers that domain only, and certifies it.
    let (walk, _) = host.workspace_index_candidates(
        &demand,
        registration_domain_lookup(registration_domain::PROVIDE_INJECT),
        WIDE,
        None,
        &never(),
    );
    assert_eq!(walk.status, EnumerationStatus::Complete);
    assert_eq!(
        registration_rows(&walk)
            .into_iter()
            .map(|(_, key, _, _)| key)
            .collect::<Vec<_>>(),
        ["provide-inject:theme", "provide-inject:user"]
    );
}

#[test]
fn registration_answers_after_edit_add_and_delete_equal_a_fresh_host() {
    let incremental = VerterHost::new_standalone(HostConfig::default());
    let mut files: BTreeMap<&str, String> = BTreeMap::new();
    let demand = IndexDemand::of([IndexSurface::Registrations]);
    let assert_equal = |incremental: &VerterHost, files: &BTreeMap<&str, String>, step: &str| {
        let fresh = VerterHost::new_standalone(HostConfig::default());
        for (id, source) in files.iter().rev() {
            upsert_file(&fresh, id, source, FileLanguage::vue());
        }
        for domain in [
            registration_domain::PROVIDE_INJECT,
            registration_domain::STORE,
            registration_domain::EVENT,
        ] {
            let lookup = registration_domain_lookup(domain);
            let (live, _) = incremental.workspace_index_candidates(
                &demand,
                lookup.clone(),
                WIDE,
                None,
                &never(),
            );
            let (clean, _) =
                fresh.workspace_index_candidates(&demand, lookup, WIDE, None, &never());
            assert_eq!(fingerprint(&live), fingerprint(&clean), "{step}: {domain}");
            assert_eq!(live.status, EnumerationStatus::Complete, "{step}: {domain}");
        }
    };
    files.insert("/src/A.vue", REGISTRATION_SOURCE.to_string());
    upsert_file(
        &incremental,
        "/src/A.vue",
        REGISTRATION_SOURCE,
        FileLanguage::vue(),
    );
    assert_equal(&incremental, &files, "initial");

    let edited = REGISTRATION_SOURCE.replace("inject('user')", "inject('account')");
    files.insert("/src/A.vue", edited.clone());
    upsert_file(&incremental, "/src/A.vue", &edited, FileLanguage::vue());
    assert_equal(&incremental, &files, "edit");

    files.insert("/src/B.vue", REGISTRATION_SOURCE.to_string());
    upsert_file(
        &incremental,
        "/src/B.vue",
        REGISTRATION_SOURCE,
        FileLanguage::vue(),
    );
    assert_equal(&incremental, &files, "add");

    files.remove("/src/A.vue");
    incremental.remove("/src/A.vue");
    assert_equal(&incremental, &files, "delete");
}

#[test]
fn registrations_are_produced_on_demand_under_a_scope_that_computes_no_template() {
    use verter_semantic::analysis::AnalysisScope;

    let host = VerterHost::new_standalone(HostConfig {
        analysis_scope: Some(AnalysisScope::BUILD),
        ..HostConfig::default()
    });
    upsert_file(
        &host,
        "/src/A.vue",
        REGISTRATION_SOURCE,
        FileLanguage::vue(),
    );
    let demand = IndexDemand::of([IndexSurface::Registrations]);
    let (page, _) = host.workspace_index_candidates(
        &demand,
        registration_domain_lookup(registration_domain::EVENT),
        WIDE,
        None,
        &never(),
    );
    assert_eq!(page.status, EnumerationStatus::Complete);
    assert_eq!(
        registration_rows(&page)
            .into_iter()
            .map(|(_, key, is_usage, _)| (key, is_usage))
            .collect::<Vec<_>>(),
        [
            ("event:close".to_string(), true),
            ("event:save".to_string(), false)
        ],
        "a cold carrier's template facts are produced for the demand"
    );
}
