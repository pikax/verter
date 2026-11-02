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
    assert_eq!(edited.unchanged, 2);
    assert_eq!(edited.population_checks, 3, "the witness read is counted");
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
    for partition in [
        IndexDemandPartition::Bindings,
        IndexDemandPartition::Components,
        IndexDemandPartition::Imports,
        IndexDemandPartition::StyleSelectors,
    ] {
        assert_eq!(
            snapshot.contributions_in_partition(&partition.id()).count(),
            0,
            "{partition:?}"
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
    for slot in snapshot.contributions_in_partition(&IndexDemandPartition::Components.id()) {
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

    upsert_file(
        &host,
        "/src/A.vue",
        &vue_source("--brand"),
        FileLanguage::svelte(),
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
            partition: IndexDemandPartition::CssVariables,
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
        &IndexDemand::of([]),
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
fn a_partition_the_analysis_scope_does_not_compute_is_unsupported_and_uncovered() {
    use verter_semantic::analysis::AnalysisScope;

    let source = "<script setup>\nimport Foo from './Foo.vue';\nconst alpha = 1;\n</script>\n<template><Foo :style=\"{ '--brand': 'x' }\" /></template>\n<style>\n.klass { --brand: blue; }\n</style>";
    let with_scope = |scope: AnalysisScope| {
        let host = VerterHost::new_standalone(HostConfig {
            analysis_scope: Some(scope),
            ..HostConfig::default()
        });
        upsert_file(&host, "/src/A.vue", source, FileLanguage::vue());
        host
    };
    let cases = [
        (
            IndexDemandPartition::CssVariables,
            css_keys("--brand"),
            [
                AnalysisScope::ESSENTIAL,
                AnalysisScope::IMPORTS,
                AnalysisScope::NONE,
            ],
        ),
        (
            IndexDemandPartition::StyleSelectors,
            selector_keys("klass"),
            [
                AnalysisScope::ESSENTIAL,
                AnalysisScope::IMPORTS,
                AnalysisScope::NONE,
            ],
        ),
        (
            IndexDemandPartition::Components,
            Lookup::Keys(vec![PostingKey::exact(PartitionFamily::Components, "Foo")]),
            [
                AnalysisScope::ESSENTIAL,
                AnalysisScope::IMPORTS,
                AnalysisScope::NONE,
            ],
        ),
        (
            IndexDemandPartition::Bindings,
            Lookup::Keys(vec![PostingKey::exact(PartitionFamily::Bindings, "alpha")]),
            [
                AnalysisScope::IMPORTS,
                AnalysisScope::NONE,
                AnalysisScope::NONE,
            ],
        ),
        (
            IndexDemandPartition::Imports,
            Lookup::Keys(vec![PostingKey::reverse(
                PartitionFamily::Imports,
                "./Foo.vue",
            )]),
            [
                AnalysisScope::NONE,
                AnalysisScope::NONE,
                AnalysisScope::NONE,
            ],
        ),
    ];
    for (partition, lookup, narrowed) in cases {
        let demand = IndexDemand::of([partition]);
        let full = with_scope(AnalysisScope::LSP);
        let (page, _) =
            full.workspace_index_candidates(&demand, lookup.clone(), WIDE, None, &never());
        assert_eq!(page.status, EnumerationStatus::Complete, "{partition:?}");
        assert!(!page.candidates.is_empty(), "{partition:?}");

        for scope in narrowed {
            let host = with_scope(scope);
            let (page, work) =
                host.workspace_index_candidates(&demand, lookup.clone(), WIDE, None, &never());
            assert!(
                is_partial_with(&page, &PartialCause::UnsupportedCoverage),
                "{partition:?} under {scope:?}: {:?}",
                page.status
            );
            assert_eq!(work.projections, 0, "no file was read for {partition:?}");
            let store = host.project_type_store().workspace_index().snapshot();
            assert_eq!(store.contribution_count(), 0);
            assert!(store.coverage(&demand.scope()).is_none());
        }
    }

    // What the scope does compute stays certified under the same demand.
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
    assert_eq!(work.population_checks, 2, "only real files are compared");
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
