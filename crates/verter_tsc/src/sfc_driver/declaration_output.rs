//! Declaration files written all at once or not at all.
//!
//! A project's declarations come from one declaration program: every
//! carrier's declaration product and every TypeScript member, staged in a
//! mirror of the project directory (`declaration_mirror`). A TypeScript
//! member that imports a carrier, by a relative path or a `paths` alias,
//! reads the carrier's product there, so a carrier type it infers is
//! published as the carrier declares it.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use verter_compiler::framework_common::vue_projection_backend::ClosureRefusal;
use verter_session_query::resolution::path_utils::{collapse_path, path_is_carrier};
use verter_span::path::InjectedPathKey;
use verter_workspace::native_fs::NativeFs;
use verter_workspace::workspace_snapshot::ProjectPayload;
use verter_workspace::VfsError;

use verter_session::{FileLanguage, ScriptSourceType};
use verter_session_query::resolution::{
    ProjectOwnership, ResolutionContext, ResolvePhase, ResolveRequestKind,
};
use verter_workspace::WorkspaceRead;

use super::declaration_mirror::{DeclarationMirror, MirrorBuilder};
use super::project_graph::{BuildMode, DriverProject, GraphLoad};
use super::project_program::ProjectCheck;
use super::DriverRequest;
use crate::checker::{one_indexed_position, EmitOptions};
use crate::reporter::{Diagnostic, VerterCode};

/// Staged declaration files of one project. Its staging and final
/// directories are private.
pub struct AtomicDeclarationOutput {
    fs: NativeFs,
    container: PathBuf,
    staging: PathBuf,
    final_dir: PathBuf,
    preserve_backup: bool,
}

impl AtomicDeclarationOutput {
    pub fn stage(final_dir: &std::path::Path) -> std::io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        // Resolve relative paths now so changing cwd cannot redirect a commit.
        let final_dir = if final_dir.is_absolute() {
            final_dir.to_path_buf()
        } else {
            std::env::current_dir()?.join(final_dir)
        };
        let final_dir = PathBuf::from(verter_span::path::canonicalize_path(path_str(&final_dir)?));
        let parent = final_dir.parent().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "output directory needs a parent",
            )
        })?;
        let fs = NativeFs::new();
        fs.create_dir_all(path_str(parent)?).map_err(io_error)?;
        let container = loop {
            let candidate = parent.join(format!(
                ".verter-declarations-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs.create_dir(path_str(&candidate)?).map_err(io_error) {
                Ok(()) => break candidate,
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        };
        let staging = container.join("output");
        let output = Self {
            fs,
            container,
            staging,
            final_dir,
            preserve_backup: false,
        };
        output
            .fs
            .create_dir_all(path_str(&output.staging)?)
            .map_err(io_error)?;
        Ok(output)
    }
    pub fn staging_dir(&self) -> &std::path::Path {
        &self.staging
    }
    /// Replace the output directory with the staged files, returning absolute,
    /// normalized paths. Called only when the project has no declaration diagnostic.
    pub fn commit(self) -> std::io::Result<Vec<std::path::PathBuf>> {
        self.commit_with(NativeFs::rename)
    }

    fn commit_with(
        mut self,
        mut rename: impl FnMut(&NativeFs, &str, &str) -> Result<(), VfsError>,
    ) -> io::Result<Vec<PathBuf>> {
        let mut relative_files = Vec::new();
        collect_files(&self.fs, &self.staging, &self.staging, &mut relative_files)?;
        relative_files.sort();
        let backup = self.container.join("previous");
        let had_previous = self.fs.is_dir(path_str(&self.final_dir)?);
        // Never replace a user file where a directory was expected.
        if !had_previous && self.fs.file_exists(path_str(&self.final_dir)?) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "output path is a file",
            ));
        }
        if had_previous {
            rename(&self.fs, path_str(&self.final_dir)?, path_str(&backup)?).map_err(io_error)?;
        }
        if let Err(error) = rename(
            &self.fs,
            path_str(&self.staging)?,
            path_str(&self.final_dir)?,
        ) {
            if had_previous {
                if let Err(restore) =
                    rename(&self.fs, path_str(&backup)?, path_str(&self.final_dir)?)
                {
                    // Retain the old output for recovery rather than delete it on drop.
                    self.preserve_backup = true;
                    return Err(io::Error::other(format!("{error}; restoring output failed: {restore}; previous output retained at {}", backup.display())));
                }
            }
            return Err(io_error(error));
        }
        Ok(relative_files
            .into_iter()
            .map(|path| self.final_dir.join(path))
            .collect())
    }
    /// Remove the staging directory and publish nothing.
    pub fn discard(self) {
        drop(self);
    }
}

impl Drop for AtomicDeclarationOutput {
    fn drop(&mut self) {
        if !self.preserve_backup {
            let _ = self.fs.delete_dir_all(&self.container.to_string_lossy());
        }
    }
}

fn path_str(path: &Path) -> io::Result<&str> {
    path.to_str()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "output path is not UTF-8"))
}

fn io_error(error: VfsError) -> io::Error {
    match error {
        VfsError::Io(error) => error,
        VfsError::NotFound(path) => io::Error::new(io::ErrorKind::NotFound, path),
        error => io::Error::other(error),
    }
}

fn collect_files(
    fs: &NativeFs,
    root: &Path,
    directory: &Path,
    files: &mut Vec<PathBuf>,
) -> io::Result<()> {
    for entry in fs.read_dir(path_str(directory)?).map_err(io_error)? {
        let path = PathBuf::from(entry.path);
        if entry.is_dir {
            collect_files(fs, root, &path, files)?;
        } else {
            files.push(
                path.strip_prefix(root)
                    .map_err(io::Error::other)?
                    .to_path_buf(),
            );
        }
    }
    Ok(())
}

/// Publish the declarations of every checked project of `load`, one output
/// directory per project. `checks` are in `load.graph.projects` order.
///
/// A project publishes all of its declarations or none: every carrier needs
/// a publishable declaration product, and the declaration stage must report
/// nothing. Otherwise the reasons become diagnostics and the output
/// directory is left as it was. Returns the files written.
///
/// A project in `not_driven` was not checked by a driver program (the
/// engine build wrote its output, or the run refused it) and publishes
/// nothing here.
pub(crate) fn publish_declarations(
    workspace: &dyn WorkspaceRead,
    request: &DriverRequest,
    load: &GraphLoad,
    checks: &mut [ProjectCheck],
    not_driven: &std::collections::HashSet<PathBuf>,
) -> Vec<PathBuf> {
    let fs = NativeFs::new();
    let targets: Vec<Option<Result<PathIdentity, String>>> = load
        .graph
        .projects
        .iter()
        .map(|project| {
            (publishes(request, project) && !not_driven.contains(&project.config)).then(|| {
                let directory = output_directory(request, load, project, &fs)?;
                // Publishing replaces the whole directory: one that holds the
                // project's config or members is refused before any other
                // project compares against it.
                let holds_sources = std::iter::once(project.config.as_path())
                    .chain(
                        project
                            .membership
                            .materialized_files
                            .iter()
                            .map(|file| Path::new(file.as_str())),
                    )
                    .any(|source| PathIdentity::of(source, &fs).is_within(&directory));
                if holds_sources {
                    return Err(format!(
                        "the output directory '{}' holds the project's own sources",
                        directory.real.display()
                    ));
                }
                Ok(directory)
            })
        })
        .collect();
    let mut emitted = Vec::new();
    for (index, (project, check)) in load
        .graph
        .projects
        .iter()
        .zip(checks.iter_mut())
        .enumerate()
    {
        let Some(target) = &targets[index] else {
            continue;
        };
        let config = check.config.to_string();
        let refusal = |reason: String| {
            Diagnostic::verter(
                config.clone(),
                1,
                1,
                VerterCode::PublicApiProjectionFailure,
                format!("Declarations were not published: {reason}"),
            )
        };
        let output_dir = match target {
            Ok(output_dir) => output_dir,
            Err(reason) => {
                let diagnostic = refusal(reason.clone());
                check.diagnostics.push(diagnostic);
                continue;
            }
        };
        // Filesystem operations act on the real directory.
        let final_dir = &output_dir.real;
        // Publishing replaces the whole directory, so no other project's
        // directory may be it, inside it, or around it.
        if let Some(other) = load
            .graph
            .projects
            .iter()
            .zip(&targets)
            .find_map(|(other, target)| {
                (other.config != project.config
                    && matches!(target, Some(Ok(dir)) if dir.overlaps(output_dir)))
                .then_some(other)
            })
        {
            let diagnostic = refusal(format!(
                "the output directory '{}' overlaps the output directory of '{}'",
                final_dir.display(),
                other.config.display()
            ));
            check.diagnostics.push(diagnostic);
            continue;
        }
        if check.engine_failure.is_some() {
            // The failure is reported already; a project that was not
            // checked publishes nothing.
            continue;
        }
        if check.carriers.len() != project.carriers.len() {
            let diagnostic = refusal("a carrier of the project was not checked".into());
            check.diagnostics.push(diagnostic);
            continue;
        }
        let mirror = match declaration_program(workspace, load, project, check) {
            Ok(mirror) => mirror,
            Err(refusals) => {
                check.diagnostics.extend(refusals);
                continue;
            }
        };
        let output = match AtomicDeclarationOutput::stage(final_dir) {
            Ok(output) => output,
            Err(error) => {
                let diagnostic = refusal(format!(
                    "the output directory '{}' cannot be staged: {error}",
                    final_dir.display()
                ));
                check.diagnostics.push(diagnostic);
                continue;
            }
        };
        let stage = mirror.emit(
            &project.config,
            &EmitOptions {
                no_emit: false,
                declaration: true,
                declaration_dir: Some(output.staging_dir().to_path_buf()),
            },
            final_dir,
            request.tsgo_bin.as_deref(),
        );
        match stage {
            Ok((diagnostics, _)) if diagnostics.is_empty() => match output.commit() {
                Ok(files) => emitted.extend(files),
                Err(error) => {
                    let diagnostic = refusal(format!(
                        "the output directory '{}' cannot be replaced: {error}",
                        final_dir.display()
                    ));
                    check.diagnostics.push(diagnostic);
                }
            },
            Ok((diagnostics, _)) => {
                output.discard();
                check.diagnostics.extend(diagnostics);
            }
            Err(error) => {
                output.discard();
                check.engine_failure = Some(error.message);
            }
        }
    }
    emitted
}

/// Whether `project` publishes declarations in this run. A project whose own
/// options set `noEmit` writes nothing, as the TypeScript compiler does
/// under `--declaration`; a solution-only project has nothing to declare.
pub(crate) fn publishes(request: &DriverRequest, project: &DriverProject) -> bool {
    request.declaration && !request.no_emit && !project.solution_only && project.emits
}

/// The project's declaration directory: with `-p`, `--declarationDir`, else
/// the project's resolved `outDir`; with `-b`, the project's own `outDir`.
fn output_directory(
    request: &DriverRequest,
    load: &GraphLoad,
    project: &DriverProject,
    fs: &NativeFs,
) -> Result<PathIdentity, String> {
    let requested = match request.mode {
        BuildMode::Project => request.declaration_dir.clone(),
        BuildMode::Build => None,
    };
    let configured = || {
        let config = project.config.to_str()?;
        load.published
            .snapshot
            .projects
            .iter()
            .find_map(|candidate| match &candidate.payload {
                ProjectPayload::Configured {
                    tsconfig_path,
                    compiler_options,
                    ..
                } if tsconfig_path.as_str() == config => compiler_options.emit.out_dir.clone(),
                _ => None,
            })
            .map(PathBuf::from)
    };
    let directory = requested
        .or_else(configured)
        .ok_or_else(|| match request.mode {
            BuildMode::Project => {
                "the project sets no outDir and no --declarationDir was given".to_string()
            }
            BuildMode::Build => "the project sets no outDir".to_string(),
        })?;
    let directory = if directory.is_absolute() {
        directory
    } else {
        std::env::current_dir()
            .map_err(|error| format!("the working directory cannot be read: {error}"))?
            .join(directory)
    };
    Ok(PathIdentity::of(&directory, fs))
}

/// A path under both of its identities: its spelling with `.` and `..`
/// folded, and the real path a filesystem operation on it reaches (the
/// longest existing ancestor resolved, the rest appended). A comparison
/// that holds under either identity holds, so neither a `dist/../src`
/// spelling nor a link hides one path inside another.
#[derive(Debug, Clone)]
struct PathIdentity {
    lexical: PathBuf,
    real: PathBuf,
}

impl PathIdentity {
    fn of(path: &Path, fs: &NativeFs) -> Self {
        let lexical = PathBuf::from(verter_span::path::canonicalize_path(&collapse_path(
            &path.to_string_lossy(),
        )));
        let mut existing = lexical.as_path();
        let mut missing = Vec::new();
        let real = loop {
            if let Some(real) = existing.to_str().and_then(|path| fs.realpath(path)) {
                break PathBuf::from(real);
            }
            let (Some(parent), Some(name)) = (existing.parent(), existing.file_name()) else {
                break existing.to_path_buf();
            };
            missing.push(name);
            existing = parent;
        };
        let mut real = real;
        for name in missing.into_iter().rev() {
            real.push(name);
        }
        let real = PathBuf::from(verter_span::path::canonicalize_path(
            &real.to_string_lossy(),
        ));
        Self { lexical, real }
    }

    /// True when `self` is `directory` or lies under it.
    fn is_within(&self, directory: &Self) -> bool {
        [&self.lexical, &self.real].into_iter().any(|path| {
            [&directory.lexical, &directory.real]
                .into_iter()
                .any(|directory| is_within(path, directory))
        })
    }

    /// True when one directory is the other or contains it.
    fn overlaps(&self, other: &Self) -> bool {
        self.is_within(other) || other.is_within(self)
    }
}

/// True when `path` is `directory` or lies under it, under the workspace's
/// path-equivalence policy.
pub(super) fn is_within(path: &Path, directory: &Path) -> bool {
    let path = InjectedPathKey::new(&path.to_string_lossy());
    let directory = InjectedPathKey::new(&directory.to_string_lossy());
    let (path, directory) = (path.as_str(), directory.as_str().trim_end_matches('/'));
    path == directory
        || path
            .strip_prefix(directory)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// The project's declaration program, or the reasons a carrier cannot
/// publish: every carrier's product and every TypeScript member, with each
/// specifier bound through the workspace resolver.
fn declaration_program(
    workspace: &dyn WorkspaceRead,
    load: &GraphLoad,
    project: &DriverProject,
    check: &ProjectCheck,
) -> Result<DeclarationMirror, Vec<Diagnostic>> {
    let unpublished = |reason: String| {
        vec![Diagnostic::verter(
            project.config.to_string_lossy().replace('\\', "/"),
            1,
            1,
            VerterCode::PublicApiProjectionFailure,
            format!("Declarations were not published: {reason}"),
        )]
    };
    // Specifiers resolve in this project, whatever else owns the file.
    let owner = load
        .published
        .snapshot
        .projects
        .iter()
        .find_map(|candidate| match &candidate.payload {
            ProjectPayload::Configured { tsconfig_path, .. }
                if Path::new(tsconfig_path.as_str()) == project.config =>
            {
                Some(ProjectOwnership {
                    project_root: verter_span::path::canonicalize_path(candidate.root.as_str()),
                    tsconfig_path: Some(tsconfig_path.as_str().to_string()),
                })
            }
            _ => None,
        })
        .ok_or_else(|| unpublished("the project is not in the workspace snapshot".into()))?;
    let mut program =
        MirrorBuilder::new(&project.root).map_err(|error| unpublished(error.message))?;
    let mut refusals = Vec::new();
    for carrier in &check.carriers {
        let path = Path::new(&*carrier.carrier);
        let refuse = |line: u32, col: u32, reason: String| {
            Diagnostic::verter(
                carrier.carrier.to_string(),
                line,
                col,
                VerterCode::PublicApiProjectionFailure,
                format!("Declaration was not published: {reason}"),
            )
        };
        if !path.starts_with(&project.root) {
            refusals.push(refuse(
                1,
                1,
                format!(
                    "the carrier is outside the project directory '{}'",
                    project.root.display()
                ),
            ));
            continue;
        }
        let Some(product) = carrier
            .result
            .module
            .as_ref()
            .and_then(|module| module.declarations.as_ref())
        else {
            refusals.push(refuse(
                1,
                1,
                "the carrier has no declaration product".into(),
            ));
            continue;
        };
        if !product.is_publishable() {
            // The product also records each typed refusal as a diagnostic of
            // its own; the typed refusal is worded once, here.
            let mirrors: Vec<String> = product
                .closure
                .refused
                .iter()
                .map(|refused| format!("Unpublishable declaration dependency: {refused:?}"))
                .collect();
            for diagnostic in &product.diagnostics {
                if mirrors.contains(&diagnostic.message) {
                    continue;
                }
                let (line, col) = one_indexed_position(&carrier.source, diagnostic.start as usize);
                refusals.push(refuse(line, col, diagnostic.message.clone()));
            }
            for refused in &product.closure.refused {
                refusals.push(refuse(1, 1, closure_refusal(refused)));
            }
            continue;
        }
        if let Err(reason) = program.carrier(path, carrier.source.clone(), product) {
            refusals.push(refuse(1, 1, reason));
        }
    }
    if !refusals.is_empty() {
        return Err(refusals);
    }
    let fs = NativeFs::new();
    for file in project
        .membership
        .materialized_files
        .iter()
        .filter(|file| !path_is_carrier(file.as_str()))
    {
        let path = Path::new(file.as_str());
        let declaration_file = matches!(
            workspace.classify_file(file.as_str()),
            FileLanguage::Script {
                source_type: ScriptSourceType::Dts,
                ..
            }
        );
        match fs.read_file(file.as_str()) {
            Some(text) if !declaration_file => program.member(path, &text),
            _ => program.list(path),
        }
    }
    let context = ResolutionContext {
        phase: ResolvePhase::ProviderGraph,
        kind: ResolveRequestKind::EsmImport,
    };
    program
        .bind(|importer, specifier| {
            workspace
                .resolve_import_from_project_outcome(
                    &owner,
                    None,
                    &importer.to_string_lossy().replace('\\', "/"),
                    specifier,
                    context,
                )
                .result()
                .map(|resolved| resolved.source_id.clone())
        })
        .map_err(unpublished)
}

fn closure_refusal(refused: &ClosureRefusal) -> String {
    match refused {
        ClosureRefusal::PrivateCheckingPath { text } => {
            format!("the public types name the checking path '{text}'")
        }
        ClosureRefusal::InaccessibleCapture { name } => {
            format!("the public types name '{name}', which the declaration cannot carry")
        }
        ClosureRefusal::WorkspaceOnlyImport { specifier } => {
            format!("the public types import '{specifier}', which resolves only in this workspace")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use verter_workspace::native_fs::NativeFs;

    #[test]
    fn failed_rollback_keeps_a_recoverable_previous_output() {
        let temp = tempfile::tempdir().unwrap();
        let final_dir = temp.path().join("types");
        let fs = NativeFs::new();
        fs.write_file(final_dir.join("keep.d.ts").to_str().unwrap(), "keep")
            .unwrap();
        let output = AtomicDeclarationOutput::stage(&final_dir).unwrap();
        let previous = output.container.join("previous/keep.d.ts");
        let mut renames = 0;
        assert!(output
            .commit_with(|fs, source, destination| {
                renames += 1;
                if renames > 1 {
                    return Err(VfsError::Io(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "injected rename failure",
                    )));
                }
                fs.rename(source, destination)
            })
            .is_err());
        assert_eq!(
            fs.read_file(previous.to_str().unwrap()).as_deref(),
            Some("keep")
        );
    }

    #[test]
    fn failed_publish_restores_previous_output() {
        let temp = tempfile::tempdir().unwrap();
        let final_dir = temp.path().join("types");
        let fs = NativeFs::new();
        fs.write_file(final_dir.join("keep.d.ts").to_str().unwrap(), "keep")
            .unwrap();
        let output = AtomicDeclarationOutput::stage(&final_dir).unwrap();
        let staging = output.staging_dir().to_path_buf();
        fs.write_file(staging.join("new.d.ts").to_str().unwrap(), "new")
            .unwrap();
        let mut renames = 0;
        let result = output.commit_with(|fs, source, destination| {
            renames += 1;
            if renames == 2 {
                return Err(VfsError::Io(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "injected rename failure",
                )));
            }
            fs.rename(source, destination)
        });
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(renames, 3);
        assert_eq!(
            fs.read_file(final_dir.join("keep.d.ts").to_str().unwrap())
                .as_deref(),
            Some("keep")
        );
        assert!(!fs.file_exists(final_dir.join("new.d.ts").to_str().unwrap()));
        assert!(!fs.is_dir(staging.to_str().unwrap()));
    }

    #[test]
    fn new_destination_and_independent_stages_commit_without_sharing_files() {
        let temp = tempfile::tempdir().unwrap();
        let final_dir = temp.path().join("nested/types");
        let first = AtomicDeclarationOutput::stage(&final_dir).unwrap();
        let second = AtomicDeclarationOutput::stage(&final_dir).unwrap();
        assert_ne!(first.staging_dir(), second.staging_dir());
        first.discard();
        assert!(second.commit().unwrap().is_empty());
        assert!(NativeFs::new().is_dir(final_dir.to_str().unwrap()));
    }

    #[test]
    fn commit_replaces_the_complete_output_and_removes_staging() {
        let temp = tempfile::tempdir().unwrap();
        let final_dir = temp.path().join("types");
        let fs = NativeFs::new();
        fs.write_file(final_dir.join("old.d.ts").to_str().unwrap(), "old")
            .unwrap();
        let output = AtomicDeclarationOutput::stage(&final_dir).unwrap();
        let staging = output.staging_dir().to_path_buf();
        fs.write_file(
            staging.join("nested/View.vue.d.ts").to_str().unwrap(),
            "export {};",
        )
        .unwrap();
        assert!(fs.file_exists(final_dir.join("old.d.ts").to_str().unwrap()));
        assert_eq!(
            output.commit().unwrap(),
            vec![PathBuf::from(verter_span::path::canonicalize_path(
                final_dir.to_str().unwrap()
            ))
            .join("nested/View.vue.d.ts")]
        );
        assert_eq!(
            fs.read_file(final_dir.join("nested/View.vue.d.ts").to_str().unwrap())
                .as_deref(),
            Some("export {};")
        );
        assert!(!fs.file_exists(final_dir.join("old.d.ts").to_str().unwrap()));
        assert!(!fs.is_dir(staging.to_str().unwrap()));
    }

    #[test]
    fn discard_and_drop_preserve_existing_output() {
        let temp = tempfile::tempdir().unwrap();
        let final_dir = temp.path().join("types");
        let fs = NativeFs::new();
        fs.write_file(final_dir.join("keep.d.ts").to_str().unwrap(), "keep")
            .unwrap();
        for discard in [false, true] {
            let output = AtomicDeclarationOutput::stage(&final_dir).unwrap();
            let staging = output.staging_dir().to_path_buf();
            fs.write_file(staging.join("new.d.ts").to_str().unwrap(), "new")
                .unwrap();
            if discard {
                output.discard();
            } else {
                drop(output);
            }
            assert!(!fs.is_dir(staging.to_str().unwrap()));
            assert_eq!(
                fs.read_file(final_dir.join("keep.d.ts").to_str().unwrap())
                    .as_deref(),
                Some("keep")
            );
            assert!(!fs.file_exists(final_dir.join("new.d.ts").to_str().unwrap()));
        }
    }
}
