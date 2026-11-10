//! The LSP's workspace-wide routes answer from the workspace index: the
//! authored results they always gave, the same answer incrementally as from a
//! fresh host, a walk the index could not certify surfaced as partial, and
//! bounded counted work.

use std::collections::{BTreeMap, BTreeSet};

use super::*;
use crate::css::global_classes::collect_cross_file_global_class_locations;
use crate::features::workspace_symbol::{workspace_symbols, workspace_symbols_within};
use crate::server::server_utils::{workspace_components_within, IndexShortfall};
use verter_session::workspace_index::{PartialCause, QueryBudgets};

const WIDE: QueryBudgets = QueryBudgets {
    postings: 10_000,
    records: 10_000,
};

fn put(host: &VerterHost, id: &str, source: &str, language: FileLanguage) {
    host.upsert(UpsertRequest {
        canonical_id: Some(id.to_string()),
        input_id: id.to_string(),
        source: source.into(),
        file_language: language,
        aliases: Vec::new(),
    })
    .expect("upsert");
}

fn language_of(id: &str) -> FileLanguage {
    if id.ends_with(".svelte") {
        FileLanguage::svelte()
    } else if id.ends_with(".ts") || id.ends_with(".js") {
        FileLanguage::script_ts()
    } else {
        FileLanguage::vue()
    }
}

/// Everything the three file-level routes answer, in answer order.
#[derive(Debug, PartialEq, Eq)]
struct RouteAnswers {
    components: Vec<(String, String)>,
    symbols: Vec<(String, String, Option<String>)>,
    class_references: Vec<(String, Range)>,
}

fn answers(host: &VerterHost) -> RouteAnswers {
    let components = build_workspace_components(host, "/ws/src/App.vue");
    assert_eq!(components.shortfall, None, "components");
    let symbols = workspace_symbols(host, "");
    assert_eq!(symbols.shortfall, None, "symbols");
    let classes = collect_cross_file_global_class_locations(
        host,
        None,
        "shared",
        PositionEncodingKind::UTF16,
        false,
    );
    assert_eq!(classes.shortfall, None, "class references");
    RouteAnswers {
        components: components
            .value
            .into_iter()
            .map(|component| (component.name, component.import_path))
            .collect(),
        symbols: symbols
            .value
            .into_iter()
            .map(|symbol| {
                (
                    symbol.name,
                    symbol.location.uri.as_str().to_string(),
                    symbol.container_name,
                )
            })
            .collect(),
        class_references: classes
            .value
            .into_iter()
            .map(|location| (location.uri.as_str().to_string(), location.range))
            .collect(),
    }
}

/// An edit, add, delete and rename answer exactly what a fresh host loaded with
/// the resulting files (in reverse order) answers.
#[test]
fn index_backed_routes_answer_after_each_change_like_a_fresh_host() {
    let live = VerterHost::new_standalone(HostConfig::default());
    let mut files: BTreeMap<&str, String> = BTreeMap::new();
    let fresh = |files: &BTreeMap<&str, String>| {
        let host = VerterHost::new_standalone(HostConfig::default());
        for (id, source) in files.iter().rev() {
            put(&host, id, source, language_of(id));
        }
        host
    };
    let step = |files: &BTreeMap<&str, String>, change: &str| {
        let expected = answers(&fresh(files));
        assert_eq!(answers(&live), expected, "after {change}");
    };

    let card = "<script setup>\nconst count = 1;\n</script>\n<template><div class=\"shared\"><Badge /></div></template>\n<style>\n.shared { color: red; }\n</style>";
    let badge = "<script>\nlet label = 'x';\n</script>\n<span class=\"shared\">{label}</span>";
    for (id, source) in [("/ws/src/Card.vue", card), ("/ws/src/Badge.svelte", badge)] {
        put(&live, id, source, language_of(id));
        files.insert(id, source.to_string());
    }
    step(&files, "load");
    let first = answers(&live);
    assert_eq!(
        first.components,
        [
            ("Badge".to_string(), "./Badge.svelte".to_string()),
            ("Card".to_string(), "./Card.vue".to_string()),
        ]
    );
    assert_eq!(
        first
            .class_references
            .iter()
            .map(|(uri, _)| uri.rsplit('/').next().unwrap_or_default())
            .collect::<Vec<_>>(),
        ["Badge.svelte", "Card.vue", "Card.vue"],
        "the markup use, then the global declaration and template use"
    );

    let edited = card.replace("count", "total").replace("shared", "other");
    put(&live, "/ws/src/Card.vue", &edited, FileLanguage::vue());
    files.insert("/ws/src/Card.vue", edited);
    step(&files, "edit");

    put(
        &live,
        "/ws/src/util.ts",
        "export const helper = 1;",
        FileLanguage::script_ts(),
    );
    files.insert("/ws/src/util.ts", "export const helper = 1;".to_string());
    step(&files, "add");

    let _ = live.remove("/ws/src/Badge.svelte");
    files.remove("/ws/src/Badge.svelte");
    step(&files, "delete");

    let moved = files.remove("/ws/src/Card.vue").expect("card");
    let _ = live.remove("/ws/src/Card.vue");
    put(&live, "/ws/src/Panel.vue", &moved, FileLanguage::vue());
    files.insert("/ws/src/Panel.vue", moved);
    step(&files, "rename");
    assert_eq!(
        answers(&live).components,
        [("Panel".to_string(), "./Panel.vue".to_string())]
    );
}

/// Workspace symbols keep their per-file authored order (bindings, template
/// components, then each style block's classes and custom properties) and
/// their case-insensitive substring match; plain scripts contribute nothing.
#[test]
fn workspace_symbols_keep_authored_order_and_substring_matching() {
    let host = VerterHost::new_standalone(HostConfig::default());
    put(
        &host,
        "/ws/src/Card.vue",
        "<script setup>\nconst count = 1;\nfunction bump() {}\n</script>\n<template><Badge /></template>\n<style>\n.a { --x: 1px; }\n</style>\n<style scoped>\n.b { color: red; }\n</style>",
        FileLanguage::vue(),
    );
    put(
        &host,
        "/ws/src/plain.ts",
        "export const bumpPlain = 1;",
        FileLanguage::script_ts(),
    );
    let names = |query: &str| {
        let answer = workspace_symbols(&host, query);
        assert_eq!(answer.shortfall, None);
        answer
            .value
            .into_iter()
            .map(|symbol| (symbol.name, symbol.kind, symbol.container_name))
            .collect::<Vec<_>>()
    };
    let container = |text: &str| Some(text.to_string());
    assert_eq!(
        names(""),
        [
            (
                "count".to_string(),
                SymbolKind::VARIABLE,
                container("Card.vue")
            ),
            (
                "bump".to_string(),
                SymbolKind::FUNCTION,
                container("Card.vue")
            ),
            (
                "Badge".to_string(),
                SymbolKind::CLASS,
                container("<template> in Card.vue")
            ),
            (
                ".a".to_string(),
                SymbolKind::STRING,
                container("<style> in Card.vue")
            ),
            (
                "--x".to_string(),
                SymbolKind::VARIABLE,
                container("<style> in Card.vue")
            ),
            (
                ".b".to_string(),
                SymbolKind::STRING,
                container("<style> in Card.vue")
            ),
        ]
    );
    let matched: Vec<String> = names("B").into_iter().map(|(name, ..)| name).collect();
    assert_eq!(matched, ["bump", "Badge", ".b"], "substring, any case");
}

/// Index candidates name the files, and each file's own facts keep a scoped
/// or module declaration local; only global declarations cross files.
#[test]
fn global_class_locations_keep_scoped_module_and_global_declarations_apart() {
    let host = VerterHost::new_standalone(HostConfig::default());
    for (id, source) in [
        (
            "/ws/src/A.vue",
            "<template><div class=\"btn\" /></template>\n<style scoped>\n.btn { color: red; }\n</style>",
        ),
        ("/ws/src/B.vue", "<style module>\n.btn { color: red; }\n</style>"),
        ("/ws/src/C.vue", "<style>\n.btn { color: red; }\n</style>"),
        (
            "/ws/src/D.svelte",
            "<div class=\"btn\"></div>\n<style>\n:global(.btn) { color: red; }\n</style>",
        ),
        ("/ws/src/E.ts", "export const btn = 1;"),
    ] {
        put(&host, id, source, language_of(id));
    }
    let files = |declarations_only| {
        let answer = collect_cross_file_global_class_locations(
            &host,
            None,
            "btn",
            PositionEncodingKind::UTF16,
            declarations_only,
        );
        assert_eq!(answer.shortfall, None);
        answer
            .value
            .iter()
            .map(|location| {
                let uri = location.location_file();
                uri.to_string()
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(
        files(true),
        ["C.vue", "D.svelte"],
        "global declarations only"
    );
    assert_eq!(
        files(false),
        ["A.vue", "C.vue", "D.svelte", "D.svelte"],
        "global declarations and every use"
    );
    let from_c = collect_cross_file_global_class_locations(
        &host,
        Some("/ws/src/C.vue"),
        "btn",
        PositionEncodingKind::UTF16,
        true,
    );
    assert_eq!(
        from_c
            .value
            .iter()
            .map(|location| location.location_file().to_string())
            .collect::<Vec<_>>(),
        ["D.svelte"],
        "the origin's own spans come from the same-file path"
    );
}

trait LocationFile {
    fn location_file(&self) -> &str;
}

impl LocationFile for Location {
    fn location_file(&self) -> &str {
        self.uri.as_str().rsplit('/').next().unwrap_or_default()
    }
}

/// A warm route reads no file, unrelated scripts add no component visit, and a
/// population past the budget answers partial with its positives.
#[test]
fn index_backed_routes_count_bounded_work_and_report_a_partial_walk() {
    let host = VerterHost::new_standalone(HostConfig::default());
    for id in ["/ws/src/A.vue", "/ws/src/B.vue", "/ws/src/C.svelte"] {
        put(&host, id, "<template><div /></template>", language_of(id));
    }
    let current = "/ws/src/App.vue";
    let cold = workspace_components_within(&host, current, WIDE);
    assert_eq!(cold.shortfall, None);
    assert_eq!(cold.value.len(), 3);
    assert_eq!(cold.work.reconcile.projections, 3);

    let warm = workspace_components_within(&host, current, WIDE);
    assert_eq!(
        warm.work.reconcile.projections, 0,
        "a warm route reads no file"
    );
    assert_eq!(warm.work.reconcile.population_checks, 0);
    assert_eq!(warm.work.walks, 1);
    assert_eq!(
        warm.work.query.key_lookups, 3,
        "one subject key per carrier"
    );
    assert_eq!(warm.work.query.posting_visits, 3);

    for index in 0..5 {
        put(
            &host,
            &format!("/ws/lib/unrelated{index}.ts"),
            "export const value = 1;",
            FileLanguage::script_ts(),
        );
    }
    let grown = workspace_components_within(&host, current, WIDE);
    assert_eq!(
        grown.work.reconcile.projections, 0,
        "a script holds no component slot, and no carrier is read again"
    );
    assert_eq!(
        grown.work.query, warm.work.query,
        "unrelated files add no visit"
    );
    assert_eq!(grown.value.len(), 3);

    let tight = QueryBudgets {
        postings: 2,
        records: 2,
    };
    let partial = workspace_components_within(&host, current, tight);
    assert!(
        matches!(
            &partial.shortfall,
            Some(IndexShortfall::Partial {
                cause: PartialCause::BudgetExhausted,
                ..
            })
        ),
        "{:?}",
        partial.shortfall
    );
    assert_eq!(partial.value.len(), 2, "the positives it did visit");
    assert!(partial.work.query.posting_visits <= tight.postings);
    let again = workspace_components_within(&host, current, WIDE);
    assert_eq!(
        again.shortfall, None,
        "a partial walk warms nothing partial"
    );
    assert_eq!(again.value.len(), 3);

    // The symbol route visits each added script once and no carrier again.
    let symbols = workspace_symbols_within(&host, "", WIDE);
    assert_eq!(symbols.shortfall, None);
    for index in 5..7 {
        put(
            &host,
            &format!("/ws/lib/unrelated{index}.ts"),
            "export const value = 1;",
            FileLanguage::script_ts(),
        );
    }
    let symbols = workspace_symbols_within(&host, "", WIDE);
    assert_eq!(symbols.shortfall, None);
    assert_eq!(
        symbols.work.reconcile.projections, 6,
        "two new scripts, each with a binding, selector and variable slot"
    );
    let tight_symbols = workspace_symbols_within(&host, "", tight);
    assert!(matches!(
        tight_symbols.shortfall,
        Some(IndexShortfall::Partial {
            cause: PartialCause::BudgetExhausted,
            ..
        })
    ));
}

fn service_over(host: Arc<VerterHost>) -> tower_lsp_server::LspService<VerterLanguageServer> {
    let (service, _socket) = tower_lsp_server::LspService::new(move |_client| {
        VerterLanguageServer::new(
            crate::outbound::Outbound::default(),
            LspConfig {
                host: Arc::clone(&host),
                type_provider: None,
                project_sync_mode: ProjectSyncMode::FullProject,
                type_provider_kind: crate::TypeProviderKind::Tsserver,
                type_provider_topology: crate::TypeProviderTopology::implied_by(
                    crate::TypeProviderKind::Tsserver,
                ),
                mcp_port: None,
                type_provider_reason: None,
                type_provider_advisory: None,
                suppress_imported_carrier_prewarm: false,
            },
        )
    });
    service
}

/// A surface the host cannot produce answers the retryable partial error at
/// the protocol boundary, never a list that reads as complete. A route that
/// does not read that surface stays complete.
#[tokio::test]
async fn an_uncertified_population_is_partial_at_the_protocol_boundary() {
    use verter_semantic::analysis::AnalysisScope;

    let host = Arc::new(VerterHost::new_standalone(HostConfig {
        analysis_scope: Some(AnalysisScope::ESSENTIAL),
        ..HostConfig::default()
    }));
    put(
        &host,
        "/ws/src/Card.vue",
        "<script setup>\nconst count = 1;\n</script>\n<template><Badge class=\"btn\" /></template>",
        FileLanguage::vue(),
    );
    let service = service_over(Arc::clone(&host));
    let server = service.inner();
    let retry = tower_lsp_server::jsonrpc::ErrorCode::ServerError(-32803);

    let symbols = super::super::aux_features::handle_symbol(
        server,
        WorkspaceSymbolParams {
            query: String::new(),
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        },
    )
    .await
    .expect_err("an uncertified symbol population is partial");
    assert_eq!(symbols.code, retry);
    assert_eq!(
        symbols.data.as_ref().and_then(|data| data.get("partial")),
        Some(&serde_json::json!("UnsupportedCoverage"))
    );

    let references = collect_cross_file_global_class_locations(
        &host,
        None,
        "btn",
        PositionEncodingKind::UTF16,
        false,
    );
    assert!(matches!(
        references.shortfall,
        Some(IndexShortfall::Partial {
            cause: PartialCause::UnsupportedCoverage,
            ..
        })
    ));

    // The component inventory needs no style or template analysis.
    let components = build_workspace_components(&host, "/ws/src/App.vue");
    assert_eq!(components.shortfall, None);
    assert_eq!(components.value.len(), 1);
    let overview = server
        .get_project_overview(serde_json::Value::Null)
        .await
        .expect("the overview reads no style surface");
    assert_eq!(overview.files.len(), 1);
}

/// The production editor host runs the BUILD scope, with no template analysis
/// of its own. Every route still answers complete from files no editor ever
/// compiled: template facts are produced for the demand that reads them.
#[tokio::test]
async fn build_scope_routes_answer_complete_from_cold_files() {
    use verter_semantic::analysis::AnalysisScope;

    let host = Arc::new(VerterHost::new_standalone(HostConfig {
        analysis_scope: Some(AnalysisScope::BUILD),
        ..HostConfig::default()
    }));
    put(
        &host,
        "/ws/src/Card.vue",
        "<script setup>\nimport Badge from './Badge.svelte';\nconst count = 1;\n</script>\n<template><div class=\"shared\"><Badge label=\"x\" /></div></template>\n<style>\n.shared { color: red; }\n</style>",
        FileLanguage::vue(),
    );
    put(
        &host,
        "/ws/src/Badge.svelte",
        "<script>\nexport let label = '';\n</script>\n<span class=\"shared\">{label}</span>",
        FileLanguage::svelte(),
    );
    put(
        &host,
        "/ws/src/util.ts",
        "export const helper = 1;",
        FileLanguage::script_ts(),
    );

    let components = build_workspace_components(&host, "/ws/src/App.vue");
    assert_eq!(components.shortfall, None);
    assert_eq!(
        components
            .value
            .iter()
            .map(|component| component.name.as_str())
            .collect::<Vec<_>>(),
        ["Badge", "Card"]
    );

    let symbols = workspace_symbols(&host, "");
    assert_eq!(symbols.shortfall, None);
    let names: Vec<&str> = symbols
        .value
        .iter()
        .map(|symbol| symbol.name.as_str())
        .collect();
    assert!(names.contains(&"Badge"), "the template use: {names:?}");
    assert!(names.contains(&".shared"), "the style class: {names:?}");

    let references = collect_cross_file_global_class_locations(
        &host,
        None,
        "shared",
        PositionEncodingKind::UTF16,
        false,
    );
    assert_eq!(references.shortfall, None);
    assert_eq!(
        references
            .value
            .iter()
            .map(|location| location.location_file())
            .collect::<Vec<_>>(),
        ["Badge.svelte", "Card.vue", "Card.vue"],
        "the markup use, then the global declaration and the cold template use"
    );

    let service = service_over(Arc::clone(&host));
    let server = service.inner();
    let overview = server
        .get_project_overview(serde_json::Value::Null)
        .await
        .expect("overview");
    assert_eq!(overview.files.len(), 3);
    assert_eq!(overview.stats.total_component_files, 2);
    assert_eq!(overview.stats.total_components, 1);

    let parents = server
        .get_component_parents(GetComponentParentsParams {
            uri: "file:///ws/src/Badge.svelte".to_string(),
        })
        .await
        .expect("parents");
    assert_eq!(
        parents
            .parents
            .iter()
            .map(|parent| (parent.file_path.as_str(), parent.component_name.as_str()))
            .collect::<Vec<_>>(),
        [("/ws/src/Card.vue", "Badge")],
        "the cold parent's template is produced to validate its use"
    );
}

/// The project overview, route candidates and component parents read their
/// population from the index: every tracked file with its kind, and every
/// carrier with its template component uses.
#[tokio::test]
async fn project_overview_lists_the_index_population_in_canonical_order() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    for (id, source) in [
        ("/ws/src/b.ts", "export const b = 1;"),
        ("/ws/src/App.vue", "<template><Card /><Card /></template>"),
        ("/ws/src/a.js", "export const a = 1;"),
        ("/ws/src/Card.svelte", "<div>card</div>"),
        (
            "/ws/src/Scoped.vue",
            "<template><div /></template>\n<style scoped>\ndiv { color: red; }\n</style>",
        ),
    ] {
        put(&host, id, source, language_of(id));
    }
    let service = service_over(Arc::clone(&host));
    let overview = service
        .inner()
        .get_project_overview(serde_json::Value::Null)
        .await
        .expect("overview");
    let files: Vec<(&str, &str)> = overview
        .files
        .iter()
        .map(|file| (file.path.as_str(), file.kind))
        .collect();
    assert_eq!(
        files,
        [
            ("/ws/src/App.vue", "component"),
            ("/ws/src/Card.svelte", "component"),
            ("/ws/src/Scoped.vue", "component"),
            ("/ws/src/a.js", "js"),
            ("/ws/src/b.ts", "ts"),
        ]
    );
    assert_eq!(overview.stats.total_component_files, 3);
    assert_eq!(overview.stats.total_components, 2);
    assert_eq!(overview.stats.files_with_scoped_styles, 1);
    let graph: BTreeSet<(&str, Vec<&str>)> = overview
        .component_graph
        .iter()
        .map(|edge| {
            (
                edge.file.as_str(),
                edge.uses_components.iter().map(String::as_str).collect(),
            )
        })
        .collect();
    assert_eq!(
        graph,
        [("/ws/src/App.vue", vec!["Card", "Card"])]
            .into_iter()
            .collect()
    );
}
