// Workspace symbols: aggregate symbols across all indexed Vue files.

use tower_lsp_server::lsp_types::*;
use verter_host::VerterHost;

/// Search for workspace symbols matching a query string.
///
/// Aggregates symbols from all known Vue files in the host:
/// - Component names (from template analysis)
/// - Top-level bindings (variables, functions, classes)
/// - Imported bindings
/// - CSS classes and custom properties
pub fn workspace_symbols(host: &VerterHost, query: &str) -> Vec<SymbolInformation> {
    let query_lower = query.to_lowercase();
    let file_list = host.list_files();
    let mut symbols = Vec::new();

    for (canonical_id, file_kind) in &file_list {
        if *file_kind != verter_host::FileKind::VueSfc {
            continue;
        }

        let analysis = match host.get_analysis(canonical_id) {
            Some(a) => a,
            None => continue,
        };

        let uri = canonical_id_to_uri(canonical_id);
        let uri = match uri {
            Some(u) => u,
            None => continue,
        };

        // Top-level bindings (variables, functions, classes)
        for binding in &analysis.bindings {
            if !query_lower.is_empty() && !binding.name.to_lowercase().contains(&query_lower) {
                continue;
            }
            #[allow(deprecated)]
            symbols.push(SymbolInformation {
                name: binding.name.clone(),
                kind: binding_to_symbol_kind(&binding.kind),
                tags: None,
                deprecated: None,
                location: Location {
                    uri: uri.clone(),
                    range: Range::default(), // Byte offsets not easily convertible without LineIndex
                },
                container_name: Some(short_filename(canonical_id)),
            });
        }

        // Components used in template
        if let Some(template) = &analysis.template {
            for comp in &template.components {
                if !query_lower.is_empty() && !comp.name.to_lowercase().contains(&query_lower) {
                    continue;
                }
                #[allow(deprecated)]
                symbols.push(SymbolInformation {
                    name: comp.name.clone(),
                    kind: SymbolKind::CLASS,
                    tags: None,
                    deprecated: None,
                    location: Location {
                        uri: uri.clone(),
                        range: Range::default(),
                    },
                    container_name: Some(format!("<template> in {}", short_filename(canonical_id))),
                });
            }
        }

        // CSS classes
        for style in &analysis.styles {
            if let Some(css) = &style.css {
                for class in &css.classes {
                    let display_name = format!(".{}", class.name);
                    if !query_lower.is_empty()
                        && !display_name.to_lowercase().contains(&query_lower)
                    {
                        continue;
                    }
                    #[allow(deprecated)]
                    symbols.push(SymbolInformation {
                        name: display_name,
                        kind: SymbolKind::STRING,
                        tags: None,
                        deprecated: None,
                        location: Location {
                            uri: uri.clone(),
                            range: Range::default(),
                        },
                        container_name: Some(format!(
                            "<style> in {}",
                            short_filename(canonical_id)
                        )),
                    });
                }

                // Custom properties
                for prop in &css.custom_properties {
                    if !query_lower.is_empty() && !prop.name.to_lowercase().contains(&query_lower) {
                        continue;
                    }
                    #[allow(deprecated)]
                    symbols.push(SymbolInformation {
                        name: prop.name.clone(),
                        kind: SymbolKind::VARIABLE,
                        tags: None,
                        deprecated: None,
                        location: Location {
                            uri: uri.clone(),
                            range: Range::default(),
                        },
                        container_name: Some(format!(
                            "<style> in {}",
                            short_filename(canonical_id)
                        )),
                    });
                }
            }
        }
    }

    symbols
}

/// Convert AnalyzedBindingKind to LSP SymbolKind.
fn binding_to_symbol_kind(kind: &verter_analysis::AnalyzedBindingKind) -> SymbolKind {
    use verter_analysis::AnalyzedBindingKind;
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
