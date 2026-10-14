//! The program file order authority: the order a configured program lists
//! its source files in, which orders the declarations of a symbol merged
//! across files (global interfaces and namespaces, `declare global`, module
//! augmentations).
//!
//! The order is TypeScript's: the `files` roots as listed, then the
//! `include` matches (by authored entry, in directory-walk order), each root
//! preceded by the files it pulls in — `/// <reference path>` and
//! `/// <reference types>` targets, then its imports — first visit wins.
//! `/// <reference lib>` targets are library files, listed before every
//! program file; they take no place here.
//!
//! A program's order is built on demand, only for a read that orders
//! contributors from two or more files, and kept on the
//! [`FileArtifactStore`](crate::file_artifact_store::FileArtifactStore)
//! until the workspace moves. A result ordered by it observes
//! [`FactKey::ProgramFileOrder`](verter_session_query::facts::FactKey::ProgramFileOrder);
//! the validator accepts that fact only while the published order is the
//! current workspace's and still has the observed fingerprint, so an edit
//! that reorders the program — an import added to a file that contributes
//! nothing — misses every warm result ordered by it.

use std::sync::Arc;

use rustc_hash::FxHashMap;
use verter_session_query::analysis::types::Hash16;
use verter_session_query::inputs::program_order::ProgramFileOrder;
use verter_session_query::resolution::path_utils::{join_paths, parent_dir};
use verter_workspace::{CanonicalPath, ProjectPayload, WorkspaceAccess};

/// The workspace state a published order was built from. Any change —
/// project configuration, file content (an import edit), the resolution
/// world — retires the order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProgramOrderStamp {
    snapshot_generation: u64,
    content_generation: u64,
    resolution_generation: u64,
}

impl ProgramOrderStamp {
    /// The live workspace's stamp, `None` before a snapshot is published.
    pub(crate) fn live(workspace: &dyn WorkspaceAccess) -> Option<Self> {
        let root = workspace.published_root()?;
        Some(Self {
            snapshot_generation: root.snapshot.generation.0,
            content_generation: workspace.content_generation(),
            resolution_generation: workspace.resolution_fact_generation(),
        })
    }
}

/// One program's published order and the workspace state it describes.
type PublishedOrder = (ProgramOrderStamp, Arc<ProgramFileOrder>);

/// Published program orders, one per program, each valid for one
/// [`ProgramOrderStamp`].
#[derive(Debug, Default)]
pub(crate) struct ProgramOrderIndex {
    published: parking_lot::RwLock<FxHashMap<Arc<str>, PublishedOrder>>,
}

impl ProgramOrderIndex {
    fn current(&self, program: &str, stamp: ProgramOrderStamp) -> Option<Arc<ProgramFileOrder>> {
        let published = self.published.read();
        let (at, order) = published.get(program)?;
        (*at == stamp).then(|| Arc::clone(order))
    }

    /// Publish `order` for `stamp`; the previously published fingerprint.
    fn publish(&self, stamp: ProgramOrderStamp, order: &Arc<ProgramFileOrder>) -> Option<Hash16> {
        let program = Arc::clone(order.program()?);
        self.published
            .write()
            .insert(program, (stamp, Arc::clone(order)))
            .map(|(_, previous)| previous.fingerprint())
    }

    /// The fingerprint of `program`'s order if it is published for
    /// `stamp`. `None` once the workspace has moved: the order may differ,
    /// and the next read that needs it rebuilds and republishes it.
    pub(crate) fn published_fingerprint(
        &self,
        program: &str,
        stamp: ProgramOrderStamp,
    ) -> Option<Hash16> {
        self.current(program, stamp)
            .map(|order| order.fingerprint())
    }
}

impl crate::VerterHost {
    /// The file order of the program `anchor` — the requesting file —
    /// belongs to: its most specific configured project. When no configured
    /// project owns `anchor`, the program of the canonically first of
    /// `ordered` (the files being ordered) one owns: a declaration file a
    /// program only imports (listed in no `files` / `include`) still orders
    /// within the program of a file that is listed. When none has a
    /// configured owner there is no program, and contributors have no
    /// defined order.
    pub(crate) fn program_file_order(
        &self,
        anchor: &str,
        ordered: &mut dyn Iterator<Item = &str>,
    ) -> Arc<ProgramFileOrder> {
        let workspace = self.workspace();
        let Some(root) = workspace.published_root() else {
            return Arc::new(ProgramFileOrder::unordered());
        };
        let snapshot = &root.snapshot;
        if !snapshot
            .projects
            .iter()
            .any(|project| matches!(project.payload, ProjectPayload::Configured { .. }))
        {
            return Arc::new(ProgramFileOrder::unordered());
        }
        let configured_owner = |file: &str| {
            snapshot.owners_for_file(file).into_iter().find(|&project| {
                matches!(
                    snapshot.project(project).payload,
                    ProjectPayload::Configured { .. }
                )
            })
        };
        let owner = configured_owner(anchor).or_else(|| {
            ordered
                .filter_map(|file| Some((file, configured_owner(file)?)))
                .min_by(|(left, _), (right, _)| left.cmp(right))
                .map(|(_, project)| project)
        });
        let Some(ProjectPayload::Configured {
            tsconfig_path,
            membership,
            compiler_options,
            ..
        }) = owner.map(|project| &snapshot.project(project).payload)
        else {
            return Arc::new(ProgramFileOrder::unordered());
        };
        let index = self.project_type_store().indexed().program_order_index();
        let stamp = ProgramOrderStamp::live(workspace.as_ref());
        if let Some(order) = stamp.and_then(|stamp| index.current(tsconfig_path.as_str(), stamp)) {
            return order;
        }

        let mut candidates: Vec<CanonicalPath> =
            membership.materialized_files.iter().cloned().collect();
        candidates.extend(
            workspace
                .snapshot_canonicals()
                .into_iter()
                .chain(workspace.overlay_canonicals())
                .map(|canonical| CanonicalPath::new(&canonical)),
        );
        let roots = membership.spec.program_root_files(&candidates);
        let javascript = compiler_options.js_is_member();
        // A file served from outside the store, or a module name the
        // resolver refused, makes this order this request's alone.
        let mut provisional = false;
        let order = Arc::new(ProgramFileOrder::sequence(
            Arc::from(tsconfig_path.as_str()),
            roots.iter().map(|root| Arc::from(root.as_str())),
            |file| {
                let serve = self.ensure_indexed_ready_serve_unobserved(file)?;
                provisional |= !serve.store_published;
                let references = &serve.indexed.route_inventory.program_references;
                let mut pulled: Vec<Arc<str>> = Vec::new();
                for path in &references.paths {
                    if let Some(target) =
                        self.triple_slash_path_target(workspace.as_ref(), file, path)
                    {
                        pulled.push(target);
                    }
                }
                for package in &references.types {
                    let types_package = match package.strip_prefix('@') {
                        Some(scoped) => format!("@types/{}", scoped.replacen('/', "__", 1)),
                        None => format!("@types/{package}"),
                    };
                    let target = self
                        .program_module_target(file, &types_package, &mut provisional)
                        .or_else(|| self.program_module_target(file, package, &mut provisional));
                    pulled.extend(target);
                }
                for name in &references.module_names {
                    pulled.extend(self.program_module_target(file, name, &mut provisional));
                }
                pulled.retain(|target| is_program_source(target, javascript));
                Some(pulled)
            },
        ));
        if let Some(stamp) = stamp {
            if !provisional && ProgramOrderStamp::live(workspace.as_ref()) == Some(stamp) {
                let previous = index.publish(stamp, &order);
                if previous != Some(order.fingerprint()) {
                    verter_type_engine::request_observers::push_structured_event(
                        crate::component_meta_audit::StructuredAuditEvent::ProgramFileOrder {
                            program: Arc::from(tsconfig_path.as_str()),
                            prev_fingerprint: previous,
                            new_fingerprint: order.fingerprint(),
                            file_count: u32::try_from(order.len()).unwrap_or(u32::MAX),
                        },
                    );
                }
            }
        }
        order
    }

    /// The file `/// <reference path="reference" />` in `file` names
    /// (`resolveTripleslashReference` + `getSourceFileFromReferenceWorker`):
    /// the path relative to `file`'s directory, as written when it carries
    /// an extension, else the first of `.ts`, `.tsx`, `.d.ts` that exists.
    fn triple_slash_path_target(
        &self,
        workspace: &dyn WorkspaceAccess,
        file: &str,
        reference: &str,
    ) -> Option<Arc<str>> {
        let target = join_paths(&parent_dir(file), reference);
        let has_extension = target
            .rsplit('/')
            .next()
            .is_some_and(|name| name.contains('.'));
        if has_extension {
            return workspace
                .file_exists(&target)
                .then(|| Arc::from(target.as_str()));
        }
        [".ts", ".tsx", ".d.ts"]
            .into_iter()
            .map(|extension| format!("{target}{extension}"))
            .find(|candidate| workspace.file_exists(candidate))
            .map(|candidate| Arc::from(candidate.as_str()))
    }

    /// The program file module name `name` resolves to from `file`, through
    /// the one type-route resolution policy.
    fn program_module_target(
        &self,
        file: &str,
        name: &str,
        provisional: &mut bool,
    ) -> Option<Arc<str>> {
        match self.resolve_type_dependency_canonical(file, name) {
            verter_workspace::ResolutionPublication::Admitted(admitted) => {
                admitted.result().map(|target| Arc::from(target.as_str()))
            }
            verter_workspace::ResolutionPublication::Refused(_) => {
                *provisional = true;
                None
            }
        }
    }
}

/// Whether a resolved `target` is a source file of the program: library
/// files are listed apart, JSON and (without `allowJs`) JavaScript are not
/// program sources.
fn is_program_source(target: &str, javascript: bool) -> bool {
    if verter_session_query::inputs::contributors::is_automatic_lib_canonical(target)
        || target.ends_with(".json")
    {
        return false;
    }
    let is_javascript = [".js", ".jsx", ".cjs", ".mjs"]
        .iter()
        .any(|extension| target.ends_with(extension));
    javascript || !is_javascript
}

#[cfg(test)]
#[path = "program_order_tests.rs"]
mod tests;
