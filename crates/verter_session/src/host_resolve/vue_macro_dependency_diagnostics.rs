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
//!
//! The walk to an export is one computation with two ways to read
//! ([`WalkReads`]). A compile root's request route reads through the owned
//! handoffs ([`collect_pollable`]): a resolution or an indexed read that is
//! pending hands the root's driver its producer or a dispatch step and
//! suspends the root, so the walk enters no wait inside the root's poll.
//! The profile lanes read in place on their calling thread ([`collect`]).

use std::collections::BTreeSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::project_type_store::IndexedReady;
use crate::root_continuation::RootWorkSlot;
use crate::typeinfo::vue_macro_codegen::{VueMacroCodegenOutput, VueMacroDependencyFailure};
use crate::types::{CompileDependencyGap, CompileInput, HostDiagnostic, HostSeverity};
use crate::VerterHost;
use verter_session_query::resolution::ResolveRequestKind;
use verter_workspace::ResolutionPublication;

/// The gaps a missing-root walk has found so far.
type GapSet = BTreeSet<CompileDependencyGap>;

/// This file's macro dependency diagnostics and what they prove.
#[derive(Debug, PartialEq)]
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

/// How the walk reads the files and routes it follows.
#[derive(Clone, Copy)]
enum WalkReads<'w, 'h> {
    /// On the calling thread, through the synchronous resolution and
    /// indexed-read entries.
    InPlace,
    /// Through the owned resolution and load handoffs: while a resolution is
    /// pending its producer, and while an indexed read is pending a
    /// scheduler dispatch step, is handed to the root's driver through the
    /// slot, and the walk returns `Pending`.
    Owned(&'w RootWorkSlot<'h>),
}

impl<'h> WalkReads<'_, 'h> {
    /// `canonical`'s indexed artifact, or `None` when it cannot be indexed.
    async fn indexed(self, host: &'h VerterHost, canonical: &str) -> Option<Arc<IndexedReady>> {
        let serve = match self {
            Self::InPlace => host.ensure_indexed_ready_serve(canonical),
            Self::Owned(work) => {
                let owned_load = crate::host_manage::prepared_decl::OwnedSourceLoad {
                    task: verter_execution::tasks::ExecutionScope::current(&host.task_registry)
                        .unwrap_or_else(|| host.task_registry.register_task()),
                    stop: host.cooperative_drive.cancellation().clone(),
                };
                host.offering_dispatch_steps(
                    work,
                    host.ensure_indexed_ready_serve_owning_loads(canonical, owned_load),
                )
                .await
            }
        };
        serve.map(|serve| serve.indexed)
    }

    /// `importer`'s `source` by the type-route policy
    /// ([`VerterHost::resolve_type_dependency_canonical`]).
    async fn type_route(
        self,
        host: &'h VerterHost,
        importer: &str,
        source: &str,
    ) -> ResolutionPublication<String> {
        match self {
            Self::InPlace => host.resolve_type_dependency_canonical(importer, source),
            Self::Owned(work) => {
                host.resolve_type_dependency_canonical_pollable(importer, source, work)
                    .await
            }
        }
    }

    /// `importer`'s `source` on its `TypeImport` lane alone
    /// ([`VerterHost::resolve_loaded_dependency_canonical`]).
    async fn type_lane(
        self,
        host: &'h VerterHost,
        importer: &str,
        source: &str,
    ) -> ResolutionPublication<String> {
        match self {
            Self::InPlace => host.resolve_loaded_dependency_canonical(
                importer,
                source,
                ResolveRequestKind::TypeImport,
            ),
            Self::Owned(work) => {
                host.resolve_loaded_dependency_canonical_pollable(
                    importer,
                    source,
                    ResolveRequestKind::TypeImport,
                    work,
                )
                .await
            }
        }
    }
}

/// Convert `output`'s dependency failures to diagnostics and decide whether
/// they are proved absent or incomplete, reading in place on this thread.
/// `hydration_gaps` are the gaps the compile's blocker hydration recorded.
///
/// The walk that looks for gaps runs only for a missing root, so a compile
/// with no dependency failure does no extra work.
pub(super) fn collect(
    host: &VerterHost,
    snapshot: &CompileInput,
    output: &VueMacroCodegenOutput,
    hydration_gaps: &[CompileDependencyGap],
) -> MacroDependencyVerdict {
    // In-place reads answer before their poll returns, so the walk never
    // suspends.
    verter_execution::blocking::complete_now(std::pin::pin!(collect_with(
        host,
        WalkReads::InPlace,
        snapshot,
        output,
        hydration_gaps,
    )))
}

/// [`collect`] as a pollable computation for a compile root: every
/// resolution and indexed read of the walk goes through the owned handoffs,
/// handing the root's driver its work through `work`.
pub(super) async fn collect_pollable<'h>(
    host: &'h VerterHost,
    snapshot: &CompileInput,
    output: &VueMacroCodegenOutput,
    hydration_gaps: &[CompileDependencyGap],
    work: &RootWorkSlot<'h>,
) -> MacroDependencyVerdict {
    collect_with(
        host,
        WalkReads::Owned(work),
        snapshot,
        output,
        hydration_gaps,
    )
    .await
}

async fn collect_with<'h>(
    host: &'h VerterHost,
    reads: WalkReads<'_, 'h>,
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
                    reads,
                    snapshot,
                    MissingRoot {
                        owner: *owner,
                        import_source,
                        type_name,
                    },
                    hydration_gaps,
                    &mut root_gaps,
                )
                .await;
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
                if !import_backed_surface_arm_is_missing(host, reads, owner_canonical, *owner, name)
                    .await
                {
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
async fn missing_root_gaps<'h>(
    host: &'h VerterHost,
    reads: WalkReads<'_, 'h>,
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
    let target = match reads.type_route(host, importer, root.import_source).await {
        ResolutionPublication::Refused(_) => {
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
    export_surface_verdict(
        host,
        reads,
        ExportLookup {
            canonical: &target,
            name: exported,
            depth: 0,
        },
        &mut visited,
        Some(gaps),
    )
    .await;
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
async fn import_backed_surface_arm_is_missing<'h>(
    host: &'h VerterHost,
    reads: WalkReads<'_, 'h>,
    owner_canonical: &str,
    owner: verter_type_expr::TopLevelOwnerId,
    name: &str,
) -> bool {
    let Some(indexed) = reads.indexed(host, owner_canonical).await else {
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
    match reads.type_lane(host, owner_canonical, &import.source).await {
        ResolutionPublication::Refused(_) => {
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
                export_surface_verdict(
                    host,
                    reads,
                    ExportLookup {
                        canonical: &target,
                        name: requested,
                        depth: 0,
                    },
                    &mut visited,
                    None,
                )
                .await,
                ExportSurfaceVerdict::Absent
            )
        }
    }
}

/// One export lookup: whether `canonical` exports `name`, `depth` re-export
/// routes from where the walk began.
#[derive(Clone, Copy)]
struct ExportLookup<'a> {
    canonical: &'a str,
    name: &'a str,
    depth: usize,
}

/// A step of the walk, boxed because the walk recurses through re-exports.
type WalkStep<'a> = Pin<Box<dyn Future<Output = ExportSurfaceVerdict> + Send + 'a>>;

/// Whether `lookup.canonical` exports `lookup.name`, following its
/// re-export routes.
///
/// With `gaps`, this is a missing root's walk: it also records each route
/// it cannot resolve and each file it cannot index, and it walks
/// package-backed files too. Without, a package-backed file is unknowable.
fn export_surface_verdict<'a, 'h: 'a>(
    host: &'h VerterHost,
    reads: WalkReads<'a, 'h>,
    lookup: ExportLookup<'a>,
    visited: &'a mut rustc_hash::FxHashSet<String>,
    mut gaps: Option<&'a mut GapSet>,
) -> WalkStep<'a> {
    Box::pin(async move {
        const MAX_EXPORT_ROUTE_DEPTH: usize = 8;
        let ExportLookup {
            canonical,
            name,
            depth,
        } = lookup;
        if depth > MAX_EXPORT_ROUTE_DEPTH || !visited.insert(canonical.to_string()) {
            return ExportSurfaceVerdict::Unknowable;
        }
        if gaps.is_none() && host.workspace_read().is_package_backed(canonical) {
            return ExportSurfaceVerdict::Unknowable;
        }
        let Some(indexed) = reads.indexed(host, canonical).await else {
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
            match follow_export_surface_route(
                host,
                reads,
                route,
                &mut branch,
                depth,
                gaps.as_deref_mut(),
            )
            .await
            {
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
            match follow_export_surface_route(
                host,
                reads,
                route,
                &mut branch,
                depth,
                gaps.as_deref_mut(),
            )
            .await
            {
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
    })
}

/// One re-export: `from_canonical` re-exports `name` from `source`.
struct ExportRoute<'a> {
    from_canonical: &'a str,
    source: &'a str,
    name: &'a str,
}

async fn follow_export_surface_route<'h>(
    host: &'h VerterHost,
    reads: WalkReads<'_, 'h>,
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
        reads.type_route(host, from_canonical, source).await
    } else {
        reads.type_lane(host, from_canonical, source).await
    };
    match publication {
        ResolutionPublication::Refused(_) => {
            verter_type_engine::fact_tracing::note_non_cacheable_read_fan_out(
                verter_session_query::facts::reuse::NonCacheableReadReason::UnrootableRoute,
            );
            if let Some(gaps) = gaps {
                gaps.insert(route_gap(true));
            }
            ExportSurfaceVerdict::Unknowable
        }
        publication => match publication.into_delivered().flatten() {
            Some(target) => {
                export_surface_verdict(
                    host,
                    reads,
                    ExportLookup {
                        canonical: &target,
                        name,
                        depth: depth + 1,
                    },
                    visited,
                    gaps,
                )
                .await
            }
            None => {
                if let Some(gaps) = gaps {
                    gaps.insert(route_gap(false));
                }
                ExportSurfaceVerdict::Unknowable
            }
        },
    }
}
