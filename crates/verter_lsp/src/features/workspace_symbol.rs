// Workspace symbols: search the component carriers' names through the
// workspace index.

use std::collections::BTreeSet;
use std::sync::Arc;

use tower_lsp_server::ls_types::*;
use verter_session::workspace_index::{
    Candidate, IndexDemand, IndexRow, IndexSurface, KeyFamily, Lookup, NameRole, PartitionFamily,
    QueryBudgets, StyleRole, StyleSurface, StyleSymbolKind, LEXICAL_PROJECTION_VERSION,
};
use verter_session::VerterHost;

use crate::server::server_utils::{IndexBacked, IndexRoute, WORKSPACE_INDEX_BUDGETS};

/// Search for workspace symbols matching a query string.
///
/// Answers from the workspace index over the component carriers:
/// - Top-level bindings (variables, functions, classes)
/// - Components used in templates
/// - CSS classes and custom properties defined in style blocks
///
/// A non-empty query keeps names containing it, case-insensitively. Symbols
/// are ordered by file, then bindings, template components and style blocks
/// in authored order. A walk the index could not certify leaves the answer
/// partial, with its shortfall.
pub(crate) fn workspace_symbols(
    host: &VerterHost,
    query: &str,
) -> IndexBacked<Vec<SymbolInformation>> {
    workspace_symbols_within(host, query, WORKSPACE_INDEX_BUDGETS)
}

/// One symbol's place in the answer: file, then section, then authored order.
type SymbolOrder = (Arc<str>, u8, u32, u8, u32);

pub(crate) fn workspace_symbols_within(
    host: &VerterHost,
    query: &str,
    budgets: QueryBudgets,
) -> IndexBacked<Vec<SymbolInformation>> {
    let query_lower = query.to_lowercase();
    let wanted = |name: &str| query_lower.is_empty() || name.to_lowercase().contains(&query_lower);
    // Stylesheet definitions only: markup class uses are no symbols.
    let demand = IndexDemand::of([
        IndexSurface::ComponentInventory,
        IndexSurface::ComponentUses,
        IndexSurface::Bindings,
        IndexSurface::StyleDefinitions,
        IndexSurface::CssVariables,
    ]);
    let mut route = IndexRoute::new(host, budgets);
    let carriers: BTreeSet<Arc<str>> = route
        .carriers(&demand)
        .iter()
        .map(|carrier| Arc::clone(&carrier.basis.locator))
        .collect();
    let mut every_name = |partition| {
        route.walk(
            &demand,
            Lookup::Prefix {
                partition,
                family: KeyFamily::Lexical,
                version: LEXICAL_PROJECTION_VERSION,
                prefix: Arc::from(""),
            },
        )
    };
    let bindings = every_name(PartitionFamily::Bindings);
    let uses = every_name(PartitionFamily::Components);
    let classes = every_name(PartitionFamily::StyleSelectors);
    let properties = every_name(PartitionFamily::CssVariables);

    let mut symbols: Vec<(SymbolOrder, SymbolInformation)> = Vec::new();
    let mut push =
        |candidate: &Candidate, section: (u8, u32, u8), name: String, kind, container: String| {
            let locator = &candidate.contribution.basis.locator;
            let Some(uri) = canonical_id_to_uri(locator) else {
                return;
            };
            let (group, block, member) = section;
            #[allow(deprecated)]
            let symbol = SymbolInformation {
                name,
                kind,
                tags: None,
                deprecated: None,
                location: Location {
                    uri,
                    range: Range::default(), // Byte offsets not easily convertible without LineIndex
                },
                container_name: Some(container),
            };
            symbols.push((
                (
                    Arc::clone(locator),
                    group,
                    block,
                    member,
                    candidate.record_index,
                ),
                symbol,
            ));
        };
    let in_carrier =
        |candidate: &&Candidate| carriers.contains(&candidate.contribution.basis.locator);

    for candidate in bindings.iter().filter(in_carrier) {
        let Some(IndexRow::Name(row)) = candidate.record().map(|record| &record.row) else {
            continue;
        };
        let (NameRole::Declaration, Some(kind)) = (row.role, row.declaration_kind) else {
            continue;
        };
        if !wanted(&row.spelling) {
            continue;
        }
        let file = short_filename(&candidate.contribution.basis.locator);
        push(
            candidate,
            (0, 0, 0),
            row.spelling.to_string(),
            binding_to_symbol_kind(&kind),
            file,
        );
    }

    for candidate in uses.iter().filter(in_carrier) {
        let Some(IndexRow::Component(verter_session::workspace_index::ComponentRow::Usage {
            tag,
            ..
        })) = candidate.record().map(|record| &record.row)
        else {
            continue;
        };
        if !wanted(tag) {
            continue;
        }
        let file = short_filename(&candidate.contribution.basis.locator);
        push(
            candidate,
            (1, 0, 0),
            tag.to_string(),
            SymbolKind::CLASS,
            format!("<template> in {file}"),
        );
    }

    for candidate in classes.iter().chain(&properties).filter(in_carrier) {
        let Some(IndexRow::Stylesheet(row)) = candidate.record().map(|record| &record.row) else {
            continue;
        };
        let StyleSurface::Style { block } = row.surface else {
            continue;
        };
        if row.role != StyleRole::Definition {
            continue;
        }
        let (name, kind, member) = match row.kind {
            StyleSymbolKind::Class => (format!(".{}", row.name), SymbolKind::STRING, 0),
            StyleSymbolKind::CustomProperty => (row.name.to_string(), SymbolKind::VARIABLE, 1),
            StyleSymbolKind::Id => continue,
        };
        if !wanted(&name) {
            continue;
        }
        let file = short_filename(&candidate.contribution.basis.locator);
        push(
            candidate,
            (2, block, member),
            name,
            kind,
            format!("<style> in {file}"),
        );
    }

    symbols.sort_by(|a, b| a.0.cmp(&b.0));
    route.finish(symbols.into_iter().map(|(_, symbol)| symbol).collect())
}

/// Convert AnalyzedBindingKind to LSP SymbolKind.
fn binding_to_symbol_kind(
    kind: &verter_session_query::analysis::types::AnalyzedBindingKind,
) -> SymbolKind {
    use verter_session_query::analysis::types::AnalyzedBindingKind;
    match kind {
        AnalyzedBindingKind::Const | AnalyzedBindingKind::Let | AnalyzedBindingKind::Var => {
            SymbolKind::VARIABLE
        }
        AnalyzedBindingKind::Function | AnalyzedBindingKind::AsyncFunction => SymbolKind::FUNCTION,
        AnalyzedBindingKind::Class => SymbolKind::CLASS,
    }
}

/// Convert a canonical ID to a file URI.
fn canonical_id_to_uri(canonical_id: &str) -> Option<Uri> {
    let path = if canonical_id.starts_with('/') {
        format!("file://{canonical_id}")
    } else {
        format!("file:///{canonical_id}")
    };
    path.parse().ok()
}

/// Extract short filename from a canonical ID.
fn short_filename(canonical_id: &str) -> String {
    canonical_id
        .rsplit('/')
        .next()
        .unwrap_or(canonical_id)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_short_filename() {
        assert_eq!(short_filename("/project/src/App.vue"), "App.vue");
        assert_eq!(short_filename("App.vue"), "App.vue");
    }

    #[test]
    fn test_canonical_id_to_uri_unix() {
        let uri = canonical_id_to_uri("/project/App.vue");
        assert!(uri.is_some());
    }

    #[test]
    fn test_canonical_id_to_uri_windows() {
        let uri = canonical_id_to_uri("D:/project/App.vue");
        assert!(uri.is_some());
    }
}
