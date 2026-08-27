//! Host diagnostic conversion for TypeInfo-owned Vue macro dependency facts.
//!
//! The TypeInfo producer carries typed root misses and dropped surface arms.
//! This module performs the one compile-boundary conversion to host
//! diagnostics. It never branches on generic completeness or diagnostic text.

use crate::typeinfo::vue_macro_codegen::{VueMacroCodegenOutput, VueMacroDependencyFailure};
use crate::types::{CompileInput, HostDiagnostic, HostSeverity};
use crate::VerterHost;

#[derive(Clone, Copy, PartialEq, Eq)]
enum ExportSurfaceVerdict {
    Present,
    Absent,
    Unknowable,
}

pub(super) fn collect(
    host: &VerterHost,
    snapshot: &CompileInput,
    output: &VueMacroCodegenOutput,
) -> Vec<HostDiagnostic> {
    let mut diagnostics = Vec::new();
    for failure in &output.dependency_failures {
        match failure {
            VueMacroDependencyFailure::MissingRoot {
                macro_index: _,
                owner,
                import_source,
                type_name,
                macro_span,
            } => diagnostics.push(HostDiagnostic {
                severity: HostSeverity::Error,
                code: crate::types::HOST_MISSING_MACRO_TYPE_DEP.to_string(),
                message: format!(
                    "missing macro type dependency '{}' for type '{}' in '{}'",
                    import_source, type_name, snapshot.canonical_id
                ),
                arguments: Vec::new(),
                span: macro_import_span(&snapshot.script_imports, *owner, import_source, type_name)
                    .unwrap_or_else(|| verter_span::Span::new(macro_span.0, macro_span.1)),
            }),
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
    diagnostics
}

fn macro_import_span(
    imports: &[verter_semantic::analysis::AnalyzedImport],
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
        verter_semantic::resolver_core::ResolveRequestKind::TypeImport,
    ) {
        verter_workspace::ResolutionPublication::Admitted(admitted) => {
            let Some(target) = admitted.into_result() else {
                return true;
            };
            let requested = binding.imported_name.as_deref().unwrap_or(&binding.name);
            let mut visited = rustc_hash::FxHashSet::default();
            matches!(
                export_surface_verdict(host, &target, requested, &mut visited, 0),
                ExportSurfaceVerdict::Absent
            )
        }
        verter_workspace::ResolutionPublication::Refused(_) => {
            crate::resolver_core::resolver_context::note_non_cacheable_read_fan_out(
                crate::resolver_core::resolver_context::NonCacheableReadReason::UnrootableRoute,
            );
            false
        }
    }
}

fn export_surface_verdict(
    host: &VerterHost,
    canonical: &str,
    name: &str,
    visited: &mut rustc_hash::FxHashSet<String>,
    depth: usize,
) -> ExportSurfaceVerdict {
    const MAX_EXPORT_ROUTE_DEPTH: usize = 8;
    if depth > MAX_EXPORT_ROUTE_DEPTH || !visited.insert(canonical.to_string()) {
        return ExportSurfaceVerdict::Unknowable;
    }
    if host.workspace_read().is_package_backed(canonical) {
        return ExportSurfaceVerdict::Unknowable;
    }
    let Some(indexed) = host
        .ensure_indexed_ready_serve(canonical)
        .map(|serve| serve.indexed)
    else {
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
        match follow_export_surface_route(host, canonical, source, original, &mut branch, depth) {
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
        match follow_export_surface_route(host, canonical, source, name, &mut branch, depth) {
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

fn follow_export_surface_route(
    host: &VerterHost,
    from_canonical: &str,
    source: &str,
    name: &str,
    visited: &mut rustc_hash::FxHashSet<String>,
    depth: usize,
) -> ExportSurfaceVerdict {
    match host.resolve_loaded_dependency_canonical(
        from_canonical,
        source,
        verter_semantic::resolver_core::ResolveRequestKind::TypeImport,
    ) {
        verter_workspace::ResolutionPublication::Admitted(admitted) => {
            match admitted.into_result() {
                Some(target) => export_surface_verdict(host, &target, name, visited, depth + 1),
                None => ExportSurfaceVerdict::Unknowable,
            }
        }
        verter_workspace::ResolutionPublication::Refused(_) => {
            crate::resolver_core::resolver_context::note_non_cacheable_read_fan_out(
                crate::resolver_core::resolver_context::NonCacheableReadReason::UnrootableRoute,
            );
            ExportSurfaceVerdict::Unknowable
        }
    }
}
