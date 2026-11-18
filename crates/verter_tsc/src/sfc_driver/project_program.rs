//! One configured project as one certified engine program.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use verter_session::external_ts::{
    CarrierOwnershipResolution, EngineBackend, PublishedEnvDims, TsgoEngineBackend,
    WorkspaceProjectResolver,
};
use verter_session::semantic_capability::CertifiedTypeEngineBinding;
use verter_session::sfc_projection_cache::{revisions_of, ProjectionReadSet};
use verter_session::sfc_projection_diagnostics::{
    attribute_diagnostics, DiagnosticCategory, EngineDiagnostic, ProjectionDiagnosticAttribution,
    TransformLookup,
};
use verter_session::sfc_projection_mapper::{
    MapperProjectHandle, MapperProjectSettings, MapperTransformResult,
};
use verter_session::sfc_projection_observations::{
    projection_input_basis, CertifiedProjectionBinding, SnapshotQualifiedObservation,
};
use verter_session_query::resolution::ProjectId;
use verter_span::path::{canonicalize_path, InjectedPathKey};
use verter_tsgo_api::toolchain::discovery::Resolution;
use verter_workspace::tsgo_virtual_config::{augment_tsconfig_bytes_with, VirtualConfigOverrides};
use verter_workspace::virtual_config::compute_virtual_config_identity;
use verter_workspace::WorkspaceRead;

use super::{
    per_file::{project_carrier, CarrierProjection},
    project_graph::{DriverProject, GraphLoad, ProjectGraphError},
    SfcTypeScriptDriverAdapter,
};
use crate::api_check::{EngineSession, OverlayFile, RemapKind, TypecheckError, TypecheckInputs};
use crate::reporter::{Diagnostic, Severity, VerterCode};

pub struct ProjectProgramInputs<'a> {
    pub project: &'a DriverProject,
    pub carriers: &'a [CarrierProjection],
    /// Carriers outside the membership that the program reaches by import.
    /// They are served, never rooted.
    pub imported: &'a [CarrierProjection],
    /// The surfaces another framework's lane renders for the member carriers
    /// this project does not own.
    pub surfaces: &'a [ForeignSurface],
    pub no_emit: bool,
}

/// A surface another framework's lane renders for one of its carriers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForeignSurface {
    pub carrier: PathBuf,
    /// Where the surface is served: the path the engine reaches first for a
    /// bare import of `carrier`.
    pub path: String,
    /// The rendered surface, or `None` when the lane could not render it.
    /// That lane reports the failure; the program serves an empty module.
    pub text: Option<Arc<str>>,
}

/// Asked once per project for the carriers that project does not own.
pub trait ForeignSurfaceSource {
    fn surfaces(&self, carriers: &[PathBuf]) -> Vec<ForeignSurface>;
}

/// The surfaces a project serves for its `unowned` carriers, in their order.
/// A fresh check asks `source` for every carrier. A recheck keeps each
/// `previous` surface whose carrier is not `changed` and asks again for the
/// rest only. Nothing left to ask means no call.
pub(crate) fn foreign_surfaces(
    source: Option<&dyn ForeignSurfaceSource>,
    unowned: &[PathBuf],
    previous: &[ForeignSurface],
    changed: &[PathBuf],
) -> Vec<ForeignSurface> {
    let Some(source) = source else {
        return Vec::new();
    };
    let kept = |carrier: &PathBuf| {
        if changed.contains(carrier) {
            return None;
        }
        previous.iter().find(|surface| &surface.carrier == carrier)
    };
    let ask = unowned
        .iter()
        .filter(|carrier| kept(carrier).is_none())
        .cloned()
        .collect::<Vec<_>>();
    let asked = if ask.is_empty() {
        Vec::new()
    } else {
        source.surfaces(&ask)
    };
    unowned
        .iter()
        .filter_map(|carrier| {
            kept(carrier)
                .or_else(|| asked.iter().find(|surface| &surface.carrier == carrier))
                .cloned()
        })
        .collect()
}

/// Serve the user's own config in place, adding checking roots and only the
/// three explicit checking overrides. Authored keys and extends are preserved.
pub fn project_config_bytes(
    inputs: &ProjectProgramInputs<'_>,
    ws: &dyn WorkspaceRead,
) -> (String, String) {
    let config = canonicalize_path(&inputs.project.config.to_string_lossy());
    // A JavaScript carrier's checking module is a JavaScript file. A project
    // that does not take JavaScript files does not root it, as vue-tsc's
    // `.vue` roots are not taken either; its overlay stays, so an importer
    // resolves it as it resolves any JavaScript module.
    let javascript = verter_workspace::config::load_compiler_options(ws, &config).js_is_member();
    let paths = inputs
        .carriers
        .iter()
        .filter(|c| javascript || !c.result.dialect.is_javascript())
        .map(|c| c.result.virtual_file.to_string())
        .collect::<Vec<_>>();
    let user = ws.read_file(&config).unwrap_or_else(|| Arc::from(""));
    let membership = verter_workspace::config::load_project_membership(ws, &config);
    let bytes = augment_tsconfig_bytes_with(
        &user,
        &paths,
        &VirtualConfigOverrides {
            no_emit: inputs.no_emit,
            non_composite: true,
            allow_arbitrary_extensions: true,
            effective_membership: Some(&membership),
        },
    );
    (config, bytes)
}

/// Exactly one overlay per carrier, member or imported, named by the
/// adapter's descriptor, then one per foreign surface at the path its lane
/// names. A surface its lane could not render is an empty module, as a failed
/// carrier is.
pub fn project_overlay(inputs: &ProjectProgramInputs<'_>) -> Vec<OverlayFile> {
    let carriers = inputs
        .carriers
        .iter()
        .chain(inputs.imported)
        .map(|carrier| OverlayFile {
            path: carrier.result.virtual_file.to_string(),
            content: carrier.result.text.to_string(),
            remap: RemapKind::Passthrough,
        });
    let surfaces = inputs.surfaces.iter().map(|surface| OverlayFile {
        path: surface.path.clone(),
        content: surface.text.as_deref().unwrap_or(EMPTY_MODULE).to_string(),
        remap: RemapKind::Passthrough,
    });
    carriers.chain(surfaces).collect()
}

/// What a program is served for a module that could not be produced.
const EMPTY_MODULE: &str = "export {};";

/// The observations of one project, kept separate even for shared carriers.
#[derive(Debug, Clone)]
pub struct ProjectCheck {
    pub config: Arc<str>,
    pub carriers: Vec<CarrierProjection>,
    /// Carriers outside the membership that the program reached by import,
    /// projected under this project's options, in the order they were
    /// reached. Their diagnostics are reported as tsc reports a non-root
    /// source file; they are neither roots nor published.
    pub imported_carriers: Vec<CarrierProjection>,
    /// Member carriers the mapper does not own (another framework's), in
    /// graph order. They are neither projected nor given to the engine.
    pub unowned_carriers: Vec<PathBuf>,
    /// The surfaces served for `unowned_carriers`, in their order.
    pub foreign_surfaces: Vec<ForeignSurface>,
    /// Native layout warnings, including carriers skipped to preserve real files.
    pub diagnostics: Vec<Diagnostic>,
    pub binding: Option<CertifiedProjectionBinding>,
    pub raw: Vec<EngineDiagnostic>,
    pub attributed: Vec<ProjectionDiagnosticAttribution>,
    pub engine_failure: Option<String>,
}

impl ProjectCheck {
    /// Every carrier the program checked: the members, then the carriers it
    /// reached by import.
    pub fn checked_carriers(&self) -> impl Iterator<Item = &CarrierProjection> {
        self.carriers.iter().chain(&self.imported_carriers)
    }
}

struct ProjectTransforms<'a> {
    carriers: &'a [CarrierProjection],
    imported: &'a [CarrierProjection],
    workspace: Option<&'a dyn WorkspaceRead>,
}
impl TransformLookup for ProjectTransforms<'_> {
    fn result_for_virtual(&self, path: &str) -> Option<Arc<MapperTransformResult>> {
        let key = InjectedPathKey::new(path);
        self.carriers
            .iter()
            .chain(self.imported)
            .find(|c| InjectedPathKey::new(&c.result.virtual_file) == key)
            .map(|c| c.result.clone())
    }

    fn is_authored(&self, path: &str) -> bool {
        // Generated overlays never establish authored provenance. All other
        // paths must be present in the retained workspace, including imports
        // outside the configured root inventory and related-information files.
        self.result_for_virtual(path).is_none()
            && self
                .workspace
                .is_some_and(|workspace| workspace.file_exists(path))
    }
}

/// Graph-owned missing-reference diagnostics share the engine attribution key.
/// Rendering and engine-copy suppression must use this same representation.
pub(crate) fn missing_reference_diagnostics(
    problems: &[ProjectGraphError],
) -> Vec<ProjectionDiagnosticAttribution> {
    let raw = problems
        .iter()
        .filter_map(|problem| {
            let ProjectGraphError::MissingReference { from, to } = problem else {
                return None;
            };
            Some(EngineDiagnostic {
                file: Arc::from(canonicalize_path(&from.to_string_lossy())),
                start: 0,
                length: 0,
                code: 6053,
                category: DiagnosticCategory::Error,
                message: format!(
                    "File '{}' not found.",
                    canonicalize_path(&to.to_string_lossy())
                ),
                related: Vec::new(),
            })
        })
        .collect::<Vec<_>>();
    attribute_diagnostics(
        &raw,
        &ProjectTransforms {
            carriers: &[],
            imported: &[],
            workspace: None,
        },
    )
}

/// One project's engine program and mapper project, kept open between checks
/// so a recheck reuses its engine process and its projections.
pub(crate) struct LiveProgram {
    handle: MapperProjectHandle,
    settings: MapperProjectSettings,
    /// `None` after the engine failed; the next check opens a new one.
    session: Option<EngineSession>,
    /// Engine sessions opened so far; the current one serves under this ordinal.
    sessions_opened: u64,
    /// The current session's certification. A check advances that program
    /// only while its input basis holds.
    certified: Option<CertifiedTypeEngineBinding>,
}

impl LiveProgram {
    /// What the open engine session read from disk since it opened. Empty
    /// when no session is open.
    pub(crate) fn host_reads(&self) -> Vec<String> {
        self.session
            .as_ref()
            .map(EngineSession::host_reads)
            .unwrap_or_default()
    }

    /// Close the engine session and the mapper project.
    pub(crate) fn close(mut self, driver: &SfcTypeScriptDriverAdapter) {
        if let Some(session) = self.session.take() {
            session.close();
        }
        let _ = driver.mapper.close_project(&self.handle);
    }
}

/// What changed since a project's previous check.
pub(crate) struct ProjectChanges<'a> {
    /// The previous check, whose projections are reused.
    pub(crate) previous: &'a ProjectCheck,
    /// Carriers to project again.
    pub(crate) carriers: &'a [PathBuf],
    /// Changed paths the engine must read again.
    pub(crate) paths: &'a [String],
}

/// Check one project once: open its program, collect, close.
pub(crate) fn check_project(
    driver: &SfcTypeScriptDriverAdapter,
    load: &GraphLoad,
    project: &DriverProject,
    engine: Option<&Resolution>,
    // Build each carrier's declaration product: the project publishes.
    declarations: bool,
    surfaces: Option<&dyn ForeignSurfaceSource>,
) -> ProjectCheck {
    let mut live = None;
    let check = check_live_project(
        driver,
        load,
        project,
        engine,
        declarations,
        surfaces,
        &mut live,
        None,
    );
    if let Some(live) = live {
        live.close(driver);
    }
    check
}

/// Check one project through its live program. With `changes`, only the
/// listed carriers are projected again, and the program is advanced in its
/// running engine when its input basis held. It is opened anew when the basis
/// or the mapper settings moved, or when the previous session failed. The
/// surfaces of foreign carriers that did not change are kept as well, and a
/// program advanced in place keeps serving the carriers it reached by import.
#[allow(clippy::too_many_arguments)]
pub(crate) fn check_live_project(
    driver: &SfcTypeScriptDriverAdapter,
    load: &GraphLoad,
    project: &DriverProject,
    engine: Option<&Resolution>,
    // Build each carrier's declaration product: the project publishes.
    declarations: bool,
    surfaces: Option<&dyn ForeignSurfaceSource>,
    live: &mut Option<LiveProgram>,
    changes: Option<ProjectChanges<'_>>,
) -> ProjectCheck {
    let config: Arc<str> = Arc::from(canonicalize_path(&project.config.to_string_lossy()));
    let (owned, unowned): (Vec<&PathBuf>, Vec<&PathBuf>) = project
        .carriers
        .iter()
        .partition(|carrier| driver.mapper.owns(&carrier.to_string_lossy()));
    let mut check = ProjectCheck {
        config: config.clone(),
        carriers: Vec::new(),
        imported_carriers: Vec::new(),
        unowned_carriers: unowned.into_iter().cloned().collect(),
        foreign_surfaces: Vec::new(),
        diagnostics: Vec::new(),
        binding: None,
        raw: Vec::new(),
        attributed: Vec::new(),
        engine_failure: None,
    };
    if project.solution_only {
        if let Some(live) = live.take() {
            live.close(driver);
        }
        return check;
    }
    let engine = engine.expect("a configured checking project resolves its engine before binding");
    let program = ProgramInputs {
        driver,
        load,
        project,
        engine,
        config: &config,
        owned: &owned,
        declarations,
        surfaces,
    };
    if let Err(error) = program.check(live, changes, &mut check) {
        check.engine_failure = Some(error.message);
    }
    check
}

/// The fixed inputs of one project's check.
struct ProgramInputs<'a> {
    driver: &'a SfcTypeScriptDriverAdapter,
    load: &'a GraphLoad,
    project: &'a DriverProject,
    engine: &'a Resolution,
    config: &'a Arc<str>,
    /// The member carriers the mapper owns, in graph order.
    owned: &'a [&'a PathBuf],
    /// Build each carrier's declaration product: the project publishes.
    declarations: bool,
    /// Serves the member carriers of another framework.
    surfaces: Option<&'a dyn ForeignSurfaceSource>,
}

impl ProgramInputs<'_> {
    fn check(
        &self,
        live: &mut Option<LiveProgram>,
        changes: Option<ProjectChanges<'_>>,
        check: &mut ProjectCheck,
    ) -> Result<(), TypecheckError> {
        let Self {
            driver,
            load,
            project,
            engine,
            config,
            owned,
            declarations,
            surfaces,
        } = *self;
        let handle = MapperProjectHandle::of_config(config);
        let generation = driver.admission.restart(&handle).max(1);
        let id = load
            .project_id(&project.config)
            .ok_or_else(|| TypecheckError::new("project is absent from the published snapshot"))?;
        let dims = PublishedEnvDims::new(&load.published, driver.workspace.as_ref());
        let resolver = WorkspaceProjectResolver::new(
            &load.published.snapshot,
            driver.workspace.as_ref(),
            engine.version_string.as_str(),
            &dims,
            load.published.ownership_ready,
        );
        let mut binding = None;
        let mut carriers = Vec::new();
        for carrier in owned.iter().copied() {
            let canonical = canonicalize_path(&carrier.to_string_lossy());
            match resolver.resolve_in_project(id, &canonical) {
                CarrierOwnershipResolution::Bound(bound) => {
                    binding.get_or_insert(bound);
                    carriers.push(carrier);
                }
                CarrierOwnershipResolution::Ambiguous { .. } => {
                    check.diagnostics.push(path_conflict_warning(canonical));
                }
                state => {
                    return Err(TypecheckError::new(format!(
                        "Cannot bind carrier '{canonical}' to '{config}': {}",
                        ownership_failure(&state)
                    )))
                }
            }
        }
        // Config diagnostics also belong to projects with no source members.
        let binding = match binding {
            Some(binding) => binding,
            None => match resolver.resolve_configured_project(id) {
                CarrierOwnershipResolution::Bound(bound) => bound,
                state => {
                    return Err(TypecheckError::new(format!(
                        "Cannot bind configured project '{config}': {}",
                        ownership_failure(&state)
                    )))
                }
            },
        };
        let bound = TsgoEngineBackend::new(engine.version_string.as_str())
            .ensure_project(binding.ensure_project_request())
            .map_err(|error| {
                use verter_session::external_ts::EngineError;
                let reason = match error {
                    EngineError::EnsureFailed(reason) | EngineError::Unavailable(reason) => {
                        reason.to_string()
                    }
                    EngineError::SnapshotMismatch { required, actual } => {
                        format!("snapshot {required} was required, but the engine has {actual}")
                    }
                };
                TypecheckError::new(format!("Cannot ensure project '{config}': {reason}"))
            })?;
        let settings = MapperProjectSettings::new(
            config.clone(),
            project.custom_elements.clone(),
            Vec::new(),
            vec![config.clone()],
        )
        .with_declarations(declarations);
        if live
            .as_ref()
            .is_some_and(|live| live.settings.identity != settings.identity)
        {
            if let Some(stale) = live.take() {
                stale.close(driver);
            }
        }
        if live.is_none() {
            driver
                .mapper
                .open_project(handle.clone(), settings.clone())
                .map_err(|error| {
                    use verter_session::sfc_projection_mapper::MapperError;
                    let reason = match error {
                        MapperError::UnknownProject(_) => "project is not registered",
                        MapperError::ProjectAlreadyOpen(_) => "project is already open",
                    };
                    TypecheckError::new(format!("Cannot open mapper project '{config}': {reason}"))
                })?;
            *live = Some(LiveProgram {
                handle: handle.clone(),
                settings: settings.clone(),
                session: None,
                sessions_opened: 0,
                certified: None,
            });
        }
        let live = live.as_mut().expect("the mapper project is open");
        let mut reprojected = Vec::new();
        check.carriers = carriers
            .into_iter()
            .map(|carrier| {
                let canonical = canonicalize_path(&carrier.to_string_lossy());
                let kept = changes.as_ref().and_then(|changes| {
                    if changes.carriers.contains(carrier) {
                        return None;
                    }
                    changes
                        .previous
                        .carriers
                        .iter()
                        .find(|kept| *kept.carrier == *canonical)
                });
                match kept {
                    Some(kept) => kept.clone(),
                    None => {
                        let projected = project_carrier(
                            &driver.mapper,
                            &handle,
                            carrier,
                            driver.workspace.as_ref(),
                        );
                        reprojected.push(projected.result.virtual_file.to_string());
                        projected
                    }
                }
            })
            .collect();
        // A foreign carrier whose surface path holds a real file keeps it.
        let mut served = Vec::new();
        if surfaces.is_some() {
            for carrier in &check.unowned_carriers {
                let canonical = canonicalize_path(&carrier.to_string_lossy());
                match resolver.resolve_in_project(id, &canonical) {
                    CarrierOwnershipResolution::Ambiguous { .. } => {
                        check.diagnostics.push(path_conflict_warning(canonical));
                    }
                    _ => served.push(carrier.clone()),
                }
            }
        }
        let (previous, changed): (&[ForeignSurface], &[PathBuf]) = match &changes {
            Some(changes) => (
                changes.previous.foreign_surfaces.as_slice(),
                changes.carriers,
            ),
            None => (&[], &[]),
        };
        check.foreign_surfaces = foreign_surfaces(surfaces, &served, previous, changed);
        // A surface asked for again is read again by a running program.
        reprojected.extend(
            check
                .foreign_surfaces
                .iter()
                .filter(|surface| !previous.contains(*surface))
                .map(|surface| surface.path.clone()),
        );
        // The virtual config's identity covers the member carriers only: an
        // imported carrier and a surface are served, never roots.
        let paths = check
            .carriers
            .iter()
            .map(|carrier| carrier.result.virtual_file.to_string())
            .collect::<Vec<_>>();
        let basis = projection_input_basis(
            config,
            compute_virtual_config_identity(driver.workspace.as_ref(), config, &paths).to_hash16(),
            &settings.identity,
        );
        let advance = live.session.is_some()
            && live
                .certified
                .as_ref()
                .is_some_and(|certified| certified.input_basis() == &basis);
        let changes = changes.filter(|_| advance);
        // A program advanced in place still holds the carriers it reached by
        // import, as it holds every source it read. Each is admitted and
        // projected again, and read again when its module moved; one deleted
        // or refused since is no longer served.
        if let Some(changes) = &changes {
            let kept = &changes.previous.imported_carriers;
            // A real file now at its module path is what the import resolves to.
            let candidates = kept
                .iter()
                .filter(|carrier| !driver.workspace.file_exists(&carrier.result.virtual_file))
                .map(|carrier| carrier.carrier.to_string());
            check.imported_carriers =
                self.admit_imported(&handle, &resolver, id, candidates, check);
            for projected in &check.imported_carriers {
                let moved = kept
                    .iter()
                    .find(|kept| kept.carrier == projected.carrier)
                    .is_none_or(|kept| {
                        kept.result.virtual_file != projected.result.virtual_file
                            || kept.result.text != projected.result.text
                    });
                if moved {
                    reprojected.push(projected.result.virtual_file.to_string());
                }
            }
        }
        let inputs = ProjectProgramInputs {
            project,
            carriers: &check.carriers,
            imported: &check.imported_carriers,
            surfaces: &check.foreign_surfaces,
            no_emit: true,
        };
        let (tsconfig_path, tsconfig_bytes) =
            project_config_bytes(&inputs, driver.workspace.as_ref());
        let mut overlay = project_overlay(&inputs);
        match (&mut live.session, changes) {
            (Some(session), Some(changes)) => {
                reprojected.extend(changes.paths.iter().cloned());
                if let Err(error) = session.update(&tsconfig_bytes, &overlay, &reprojected) {
                    if let Some(failed) = live.session.take() {
                        failed.close();
                    }
                    live.certified = None;
                    return Err(error);
                }
            }
            (session, _) => {
                if let Some(previous) = session.take() {
                    previous.close();
                }
                live.certified = None;
                *session = Some(EngineSession::open(TypecheckInputs {
                    engine: &engine.path,
                    cwd: &project.root,
                    tsconfig_path,
                    tsconfig_bytes: tsconfig_bytes.clone(),
                    files: overlay.clone(),
                })?);
                live.sessions_opened += 1;
            }
        }
        // A carrier outside the membership is reached when the program
        // resolves an import of it: the engine looks for its checking module
        // on disk and does not find it. Each such carrier is served under
        // this project's options, as vue-tsc gives every carrier its program
        // reaches a virtual file, and the program advances until it reaches
        // no new one.
        loop {
            let session = live.session.as_mut().expect("the engine session is open");
            // A real file at the module path is what the import resolved to.
            let candidates = session
                .host_reads()
                .into_iter()
                .filter_map(|read| {
                    let carrier = driver.mapper.carrier_of_checking_module(&read)?;
                    (!driver.workspace.file_exists(&read)).then_some(carrier)
                })
                .collect::<Vec<_>>();
            let reached = self.admit_imported(&handle, &resolver, id, candidates, check);
            if reached.is_empty() {
                break;
            }
            check.imported_carriers.extend(reached);
            overlay = project_overlay(&ProjectProgramInputs {
                project,
                carriers: &check.carriers,
                imported: &check.imported_carriers,
                surfaces: &check.foreign_surfaces,
                no_emit: true,
            });
            // Each new module is reported as created, so the lookups that
            // failed for it match again.
            if let Err(error) = session.update(&tsconfig_bytes, &overlay, &[]) {
                if let Some(failed) = live.session.take() {
                    failed.close();
                }
                live.certified = None;
                return Err(error);
            }
        }
        let session = live.session.as_ref().expect("the engine session is open");
        let published = CertifiedTypeEngineBinding::certify(
            &bound,
            &session.serving(live.sessions_opened),
            load.lease,
            basis,
        )
        .map_err(|_| {
            Unpublished::Failed(TypecheckError::new(format!(
                "Cannot certify project '{config}': engine capabilities have no observed version"
            )))
        })
        .and_then(|engine| {
            self.publish(
                &handle, &settings, generation, engine, session, &overlay, check,
            )
        });
        match published {
            Ok(engine) => {
                live.certified = Some(engine);
                Ok(())
            }
            // The program answered; a newer change made the answer stale. It
            // stays current for the check that change brings.
            Err(Unpublished::Overtaken(engine)) => {
                live.certified = Some(*engine);
                Err(TypecheckError::new(format!(
                    "The check of '{config}' was overtaken by a newer change"
                )))
            }
            Err(Unpublished::Failed(error)) => {
                // A program that failed to answer is not reused.
                if let Some(failed) = live.session.take() {
                    failed.close();
                }
                live.certified = None;
                Err(error)
            }
        }
    }

    /// The `candidates` the program reached by import that it does not serve
    /// yet, in order, each projected for this project. A carrier that no
    /// longer exists is not served, a member is served as one or was refused
    /// as one, and a carrier whose companion path holds a real file keeps
    /// that file and is warned about, as a member is.
    fn admit_imported(
        &self,
        handle: &MapperProjectHandle,
        resolver: &WorkspaceProjectResolver<'_>,
        id: ProjectId,
        candidates: impl IntoIterator<Item = String>,
        check: &mut ProjectCheck,
    ) -> Vec<CarrierProjection> {
        let Self {
            driver, project, ..
        } = *self;
        let workspace = driver.workspace.as_ref();
        let mut served = project
            .carriers
            .iter()
            .map(|carrier| InjectedPathKey::new(&carrier.to_string_lossy()))
            .chain(
                check
                    .checked_carriers()
                    .map(|carrier| InjectedPathKey::new(&carrier.carrier)),
            )
            // A carrier refused for its companion path is warned about once.
            .chain(
                check
                    .diagnostics
                    .iter()
                    .filter(|d| d.verter_code == Some(VerterCode::CarrierPathConflict))
                    .map(|d| InjectedPathKey::new(&d.file)),
            )
            .collect::<HashSet<_>>();
        let mut reached = Vec::new();
        for carrier in candidates {
            if !workspace.file_exists(&carrier) || !served.insert(InjectedPathKey::new(&carrier)) {
                continue;
            }
            if let CarrierOwnershipResolution::Ambiguous { .. } =
                resolver.resolve_in_project(id, &carrier)
            {
                check.diagnostics.push(path_conflict_warning(carrier));
                continue;
            }
            reached.push(project_carrier(
                &driver.mapper,
                handle,
                Path::new(&carrier),
                workspace,
            ));
        }
        reached
    }

    /// Bind the certified program, record the carriers' revisions, then
    /// collect, attribute and admit its diagnostics. Returns the certification.
    #[allow(clippy::too_many_arguments)]
    fn publish(
        &self,
        handle: &MapperProjectHandle,
        settings: &MapperProjectSettings,
        generation: u64,
        engine: CertifiedTypeEngineBinding,
        session: &EngineSession,
        overlay: &[OverlayFile],
        check: &mut ProjectCheck,
    ) -> Result<CertifiedTypeEngineBinding, Unpublished> {
        let Self { driver, config, .. } = *self;
        let certification = CertifiedProjectionBinding {
            project: handle.clone(),
            config_file: config.clone(),
            engine,
            generation,
        };
        driver.admission.bind(&certification);
        let revisions = check
            .checked_carriers()
            .map(|carrier| {
                let read_set = ProjectionReadSet {
                    carrier: carrier.carrier.clone(),
                    external_sources: Vec::new(),
                    settings_identity: settings.identity,
                };
                let revisions = revisions_of(&carrier.source, &carrier.result, None, &read_set);
                driver
                    .admission
                    .record_current(handle, &carrier.carrier, revisions);
                (
                    InjectedPathKey::new(&carrier.carrier),
                    (carrier.carrier.clone(), revisions),
                )
            })
            .collect::<HashMap<_, _>>();
        let raw = session.diagnostics(overlay).map_err(Unpublished::Failed)?;
        let attributed = attribute_diagnostics(
            &raw,
            &ProjectTransforms {
                carriers: &check.carriers,
                imported: &check.imported_carriers,
                workspace: Some(driver.workspace.as_ref()),
            },
        );
        for diagnostic in attributed {
            if let Some((carrier, revisions)) =
                revisions.get(&InjectedPathKey::new(&diagnostic.carrier))
            {
                let observation = SnapshotQualifiedObservation {
                    project: handle.clone(),
                    generation: certification.generation,
                    carrier: carrier.clone(),
                    revisions: *revisions,
                    value: diagnostic,
                };
                check.attributed.push(
                    driver
                        .admission
                        .admit(observation, false)
                        .map_err(|e| unpublished(e, &certification, "Diagnostic"))?,
                );
            } else {
                check.attributed.push(
                    driver
                        .admission
                        .admit_project(handle, certification.generation, diagnostic, false)
                        .map_err(|e| unpublished(e, &certification, "Project diagnostic"))?,
                );
            }
        }
        // The engine's whole answer, also when it holds no diagnostic: a check
        // that a newer change overtook publishes nothing.
        check.raw = driver
            .admission
            .admit_project(handle, certification.generation, raw, false)
            .map_err(|e| unpublished(e, &certification, "Project result"))?;
        let engine = certification.engine.clone();
        check.binding = Some(certification);
        Ok(engine)
    }
}

/// A carrier left to the real file at one of its companion paths.
fn path_conflict_warning(canonical: String) -> Diagnostic {
    let mut diagnostic = Diagnostic::verter(
        canonical,
        1,
        1,
        VerterCode::CarrierPathConflict,
        "Carrier was not checked because its companion path conflicts with a real file".into(),
    );
    diagnostic.severity = Severity::Warning;
    diagnostic
}

fn ownership_failure(state: &CarrierOwnershipResolution) -> &'static str {
    match state {
        CarrierOwnershipResolution::NotReady => "workspace ownership is not ready",
        CarrierOwnershipResolution::NoProject => {
            "configured project or published environment dimensions are unavailable"
        }
        CarrierOwnershipResolution::Ambiguous { .. } => {
            "project ownership conflicts with another source or project"
        }
        CarrierOwnershipResolution::Bound(_) => "project binding was unexpectedly refused",
    }
}

/// Why a certified program's results were not published.
enum Unpublished {
    /// The program could not answer; it is not reused.
    Failed(TypecheckError),
    /// A newer change restarted the project while its results were admitted.
    /// The program answered under this certification and stays live.
    Overtaken(Box<CertifiedTypeEngineBinding>),
}

/// A refused admission of `what`. A restart since the binding means a newer
/// change overtook the check; any other refusal is a failure.
fn unpublished(
    reason: verter_session::sfc_projection_observations::PublicationRefusal,
    certification: &CertifiedProjectionBinding,
    what: &str,
) -> Unpublished {
    use verter_session::sfc_projection_observations::PublicationRefusal;
    match reason {
        PublicationRefusal::StaleGeneration => {
            Unpublished::Overtaken(Box::new(certification.engine.clone()))
        }
        reason => Unpublished::Failed(TypecheckError::new(format!(
            "{what} publication refused: {}",
            publication_failure(reason)
        ))),
    }
}

fn publication_failure(
    reason: verter_session::sfc_projection_observations::PublicationRefusal,
) -> &'static str {
    use verter_session::sfc_projection_observations::PublicationRefusal;
    match reason {
        PublicationRefusal::StaleGeneration => "project generation or serving lease changed",
        PublicationRefusal::StaleRevisions => "carrier revisions changed",
        PublicationRefusal::Cancelled => "work was cancelled",
        PublicationRefusal::UnknownProject => "project has no current certification",
    }
}

#[cfg(test)]
#[path = "project_program_tests.rs"]
pub(super) mod tests;
