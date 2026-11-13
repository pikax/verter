//! Host diagnostic conversion for TypeInfo-owned Vue macro dependency facts.
//!
//! The TypeInfo producer carries typed root misses and dropped surface arms.
//! This module performs the one compile-boundary conversion to host
//! diagnostics. It never branches on generic completeness or diagnostic text.
//!
//! It also decides whether a missing root is proved absent or only not
//! established from the inputs this host holds. A missing root is
//! incomplete exactly when a concrete gap is known for it: the payload
//! lookup ended early, hydration could not resolve or load its import, or
//! a route on the way to its export does not resolve or its target cannot
//! be indexed. Both verdicts carry the same diagnostic.

use std::collections::BTreeSet;

use crate::typeinfo::vue_macro_codegen::{VueMacroCodegenOutput, VueMacroDependencyFailure};
use crate::types::{CompileDependencyGap, CompileInput, HostDiagnostic, HostSeverity};
use crate::VerterHost;

/// The gaps a missing-root walk has found so far.
type GapSet = BTreeSet<CompileDependencyGap>;

/// This file's macro dependency diagnostics and what they prove.
pub(super) struct MacroDependencyVerdict {
    /// Sorted, deduplicated. Empty when every dependency was established.
    pub(super) diagnostics: Vec<HostDiagnostic>,
    /// `Some` when every diagnosed dependency is incomplete rather than
    /// proved absent: the gaps that keep it so, sorted, deduplicated and
    /// never empty. `None` when there is no diagnostic, or when at least
    /// one diagnosed dependency is proved absent.
    pub(super) incomplete: Option<Vec<CompileDependencyGap>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ExportSurfaceVerdict {
    Present,
    Absent,
    Unknowable,
}

/// Convert `output`'s dependency failures to diagnostics and decide whether
/// they are proved absent or incomplete. `hydration_gaps` are the gaps the
/// compile's blocker hydration recorded.
///
/// The walk that looks for gaps runs only for a missing root, so a compile
/// with no dependency failure does no extra work.
pub(super) fn collect(
    host: &VerterHost,
    snapshot: &CompileInput,
    output: &VueMacroCodegenOutput,
    hydration_gaps: &[CompileDependencyGap],
) -> MacroDependencyVerdict {
    let mut diagnostics = Vec::new();
    let mut gaps = GapSet::new();
    let mut proved_absent = false;
    for failure in &output.dependency_failures {
        match failure {
            VueMacroDependencyFailure::MissingRoot {
                macro_index: _,
                owner,
                import_source,
                type_name,
                macro_span,
            }
            | VueMacroDependencyFailure::IncompleteRoot {
                macro_index: _,
                owner,
                import_source,
                type_name,
                macro_span,
            } => {
                diagnostics.push(HostDiagnostic {
                    severity: HostSeverity::Error,
                    code: crate::types::HOST_MISSING_MACRO_TYPE_DEP.to_string(),
                    message: format!(
                        "missing macro type dependency '{}' for type '{}' in '{}'",
                        import_source, type_name, snapshot.canonical_id
                    ),
                    arguments: Vec::new(),
                    span: macro_import_span(
                        &snapshot.script_imports,
                        *owner,
                        import_source,
                        type_name,
                    )
                    .unwrap_or_else(|| verter_span::Span::new(macro_span.0, macro_span.1)),
                });
                let mut root_gaps = GapSet::new();
                if matches!(failure, VueMacroDependencyFailure::IncompleteRoot { .. }) {
                    root_gaps.insert(CompileDependencyGap::Lookup {
                        import_source: import_source.clone(),
                        type_name: type_name.clone(),
                    });
                }
                missing_root_gaps(
                    host,
                    snapshot,
                    MissingRoot {
                        owner: *owner,
                        import_source,
                        type_name,
                    },
                    hydration_gaps,
                    &mut root_gaps,
                );
                if root_gaps.is_empty() {
                    proved_absent = true;
                }
                gaps.append(&mut root_gaps);
            }
            VueMacroDependencyFailure::UnresolvedSurfaceArm {
                macro_index,
                macro_owner,
                name,
                owner_canonical,
                owner,
            } => {
                if !import_backed_surface_arm_is_missing(host, owner_canonical, *owner, name) {
                    continue;
                }
                let Some(dep) = snapshot.macro_type_deps.iter().find(|dependency| {
                    dependency.macro_index == *macro_index && dependency.usage.is_surface()
                }) else {
                    continue;
                };
                proved_absent = true;
                diagnostics.push(HostDiagnostic {
                    severity: HostSeverity::Error,
                    code: crate::types::HOST_MISSING_MACRO_TYPE_DEP.to_string(),
                    message: format!(
                        "missing macro type dependency: type '{}' resolves but its surface references unresolvable type '{}' (declared in '{}') in '{}'",
                        dep.type_name, name, owner_canonical, snapshot.canonical_id
                    ),
                    arguments: Vec::new(),
                    span: macro_import_span(
                        &snapshot.script_imports,
                        *macro_owner,
                        &dep.import_source,
                        &dep.type_name,
                    )
                    .unwrap_or(dep.macro_span),
                });
            }
        }
    }
    diagnostics.sort_by(|left, right| {
        (
            (left.span.start, left.span.end),
            left.code.as_str(),
            left.message.as_str(),
        )
            .cmp(&(
                (right.span.start, right.span.end),
                right.code.as_str(),
                right.message.as_str(),
            ))
    });
    diagnostics.dedup_by(|left, right| {
        left.span == right.span && left.code == right.code && left.message == right.message
    });
    let incomplete =
        (!diagnostics.is_empty() && !proved_absent).then(|| gaps.into_iter().collect());
    MacroDependencyVerdict {
        diagnostics,
        incomplete,
    }
}

/// One missing root's authored import.
struct MissingRoot<'a> {
    owner: verter_type_expr::TopLevelOwnerId,
    import_source: &'a str,
    type_name: &'a str,
}

/// The gaps known for one missing root: the hydration gaps on its own
/// import, then each route on the way from that import to the export it
/// names that does not resolve, and each file there that cannot be
/// indexed. A root whose export surface is fully established, and lacks
/// the export, gets none.
fn missing_root_gaps(
    host: &VerterHost,
    snapshot: &CompileInput,
    root: MissingRoot<'_>,
    hydration_gaps: &[CompileDependencyGap],
    gaps: &mut GapSet,
) {
    let importer = snapshot.canonical_id.as_str();
    let route_gap = |refused: bool| CompileDependencyGap::Route {
        importer: importer.to_owned(),
        specifier: root.import_source.to_owned(),
        refused,
    };
    let target = match host.resolve_type_dependency_canonical(importer, root.import_source) {
        verter_workspace::ResolutionPublication::Refused(_) => {
            gaps.insert(route_gap(true));
            None
        }
        publication => {
            let target = publication.into_delivered().flatten();
            if target.is_none() {
                gaps.insert(route_gap(false));
            }
            target
        }
    };
    for gap in hydration_gaps {
        let related = match gap {
            CompileDependencyGap::Route {
                importer: gap_importer,
                specifier,
                ..
            } => gap_importer == importer && specifier == root.import_source,
            CompileDependencyGap::File { canonical } => target.as_ref() == Some(canonical),
            CompileDependencyGap::Lookup { .. } => false,
        };
        if related {
            gaps.insert(gap.clone());
        }
    }
    let Some(target) = target else {
        return;
    };
    // The authored local name may alias the exported one.
    let exported = snapshot
        .script_imports
        .iter()
        .filter(|import| import.owner == root.owner && import.source == root.import_source)
        .flat_map(|import| import.bindings.iter())
        .find(|binding| binding.name == root.type_name)
        .and_then(|binding| binding.imported_name.as_deref())
        .unwrap_or(root.type_name);
    let mut visited = rustc_hash::FxHashSet::default();
    export_surface_verdict(host, &target, exported, &mut visited, 0, Some(gaps));
}

fn macro_import_span(
    imports: &[verter_session_query::analysis::types::AnalyzedImport],
    owner: verter_type_expr::TopLevelOwnerId,
    import_source: &str,
    type_name: &str,
) -> Option<verter_span::Span> {
    imports
        .iter()
        .find(|import| {
            import.owner == owner
                && import.source == import_source
                && import
                    .bindings
                    .iter()
                    .any(|binding| binding.name == type_name)
        })
        .map(|import| import.span)
}

/// Exact owner/import classification for one dropped surface arm. A name with
/// no authored import binding is ambient and stays silent. An unresolved route
/// is missing; a loaded route is fatal only when its facts prove the requested
/// export absent.
fn import_backed_surface_arm_is_missing(
    host: &VerterHost,
    owner_canonical: &str,
    owner: verter_type_expr::TopLevelOwnerId,
    name: &str,
) -> bool {
    let Some(indexed) = host
        .ensure_indexed_ready_serve(owner_canonical)
        .map(|serve| serve.indexed)
    else {
        return false;
    };
    let Some(analysis) = indexed.script_analysis.as_ref() else {
        return false;
    };
    let Some((import, binding)) = analysis.imports.iter().find_map(|import| {
        (import.owner == owner)
            .then_some(import)
            .and_then(|import| {
                import
                    .bindings
                    .iter()
                    .find(|binding| binding.name == name)
                    .map(|binding| (import, binding))
            })
    }) else {
        return false;
    };
    match host.resolve_loaded_dependency_canonical(
        owner_canonical,
        &import.source,
        verter_session_query::resolution::ResolveRequestKind::TypeImport,
    ) {
        verter_workspace::ResolutionPublication::Refused(_) => {
            verter_type_engine::fact_tracing::note_non_cacheable_read_fan_out(
                verter_session_query::facts::reuse::NonCacheableReadReason::UnrootableRoute,
            );
            false
        }
        publication => {
            let Some(target) = publication.into_delivered().flatten() else {
                return true;
            };
            let requested = binding.imported_name.as_deref().unwrap_or(&binding.name);
            let mut visited = rustc_hash::FxHashSet::default();
            matches!(
                export_surface_verdict(host, &target, requested, &mut visited, 0, None),
                ExportSurfaceVerdict::Absent
            )
        }
    }
}

/// Whether `canonical` exports `name`, following its re-export routes.
///
/// With `gaps`, this is a missing root's walk: it also records each route
/// it cannot resolve and each file it cannot index, and it walks
/// package-backed files too. Without, a package-backed file is unknowable.
fn export_surface_verdict(
    host: &VerterHost,
    canonical: &str,
    name: &str,
    visited: &mut rustc_hash::FxHashSet<String>,
    depth: usize,
    mut gaps: Option<&mut GapSet>,
) -> ExportSurfaceVerdict {
    const MAX_EXPORT_ROUTE_DEPTH: usize = 8;
    if depth > MAX_EXPORT_ROUTE_DEPTH || !visited.insert(canonical.to_string()) {
        return ExportSurfaceVerdict::Unknowable;
    }
    if gaps.is_none() && host.workspace_read().is_package_backed(canonical) {
        return ExportSurfaceVerdict::Unknowable;
    }
    let Some(indexed) = host
        .ensure_indexed_ready_serve(canonical)
        .map(|serve| serve.indexed)
    else {
        if let Some(gaps) = gaps {
            gaps.insert(CompileDependencyGap::File {
                canonical: canonical.to_owned(),
            });
        }
        return ExportSurfaceVerdict::Unknowable;
    };
    let Some(exports) = indexed.export_signatures.as_ref() else {
        return ExportSurfaceVerdict::Unknowable;
    };
    let mut unknowable = false;
    for export in exports.iter().filter(|export| export.name == name) {
        let Some(source) = export.reexport_source.as_deref() else {
            return ExportSurfaceVerdict::Present;
        };
        let original = export.reexport_local.as_deref().unwrap_or(name);
        let mut branch = visited.clone();
        let route = ExportRoute {
            from_canonical: canonical,
            source,
            name: original,
        };
        match follow_export_surface_route(host, route, &mut branch, depth, gaps.as_deref_mut()) {
            ExportSurfaceVerdict::Present => return ExportSurfaceVerdict::Present,
            ExportSurfaceVerdict::Unknowable => unknowable = true,
            ExportSurfaceVerdict::Absent => {}
        }
    }
    for export in exports.iter().filter(|export| export.name == "*") {
        let Some(source) = export.reexport_source.as_deref() else {
            unknowable = true;
            continue;
        };
        let mut branch = visited.clone();
        let route = ExportRoute {
            from_canonical: canonical,
            source,
            name,
        };
        match follow_export_surface_route(host, route, &mut branch, depth, gaps.as_deref_mut()) {
            ExportSurfaceVerdict::Present => return ExportSurfaceVerdict::Present,
            ExportSurfaceVerdict::Unknowable => unknowable = true,
            ExportSurfaceVerdict::Absent => {}
        }
    }
    if unknowable {
        ExportSurfaceVerdict::Unknowable
    } else {
        ExportSurfaceVerdict::Absent
    }
}

/// One re-export: `from_canonical` re-exports `name` from `source`.
struct ExportRoute<'a> {
    from_canonical: &'a str,
    source: &'a str,
    name: &'a str,
}

fn follow_export_surface_route(
    host: &VerterHost,
    route: ExportRoute<'_>,
    visited: &mut rustc_hash::FxHashSet<String>,
    depth: usize,
    gaps: Option<&mut GapSet>,
) -> ExportSurfaceVerdict {
    let ExportRoute {
        from_canonical,
        source,
        name,
    } = route;
    let route_gap = |refused: bool| CompileDependencyGap::Route {
        importer: from_canonical.to_owned(),
        specifier: source.to_owned(),
        refused,
    };
    // The gap walk follows each step by the type-route policy, as the root
    // import is resolved, so a gap it names is one supplying can close;
    // the diagnostic walk keeps its type-lane lookup.
    let publication = if gaps.is_some() {
        host.resolve_type_dependency_canonical(from_canonical, source)
    } else {
        host.resolve_loaded_dependency_canonical(
            from_canonical,
            source,
            verter_session_query::resolution::ResolveRequestKind::TypeImport,
        )
    };
    match publication {
        verter_workspace::ResolutionPublication::Refused(_) => {
            verter_type_engine::fact_tracing::note_non_cacheable_read_fan_out(
                verter_session_query::facts::reuse::NonCacheableReadReason::UnrootableRoute,
            );
            if let Some(gaps) = gaps {
                gaps.insert(route_gap(true));
            }
            ExportSurfaceVerdict::Unknowable
        }
        publication => match publication.into_delivered().flatten() {
            Some(target) => export_surface_verdict(host, &target, name, visited, depth + 1, gaps),
            None => {
                if let Some(gaps) = gaps {
                    gaps.insert(route_gap(false));
                }
                ExportSurfaceVerdict::Unknowable
            }
        },
    }
}
