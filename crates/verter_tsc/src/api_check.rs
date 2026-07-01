//! In-memory tsgo `--api` typecheck — the verter-tsc `--noEmit` backend.
//!
//! This replaces the temp-file `tsgo --project` subprocess path for type
//! checking. The generated carriers (validation TSX, public-API stubs, ambient
//! `.d.ts` shims) and the synthetic tsconfig are fed to the engine as an
//! in-memory [`OverlaySnapshot`] — NO temp files, NO subprocess spawn, NO poll
//! loop. The engine is the gated [`TsgoClient`] (`--api`-only); a diverged /
//! unknown engine fails the wire gate at `connect` and the typecheck fail-closes
//! (empty diagnostics + a stderr note) — there is NO tsc fallback for this path.
//!
//! Membership + virtual-config materialization REUSE the shared
//! [`verter_workspace::tsgo_virtual_config`] owner
//! ([`build_virtual_overlay_snapshot`]). Every configured-project `root_file`
//! (validation TSX **and** the `.vue.ts` stubs **and** the ambient `.d.ts`
//! shims) is enumerated and queried for semantic + syntactic diagnostics; the
//! per-file UTF-16 offsets are mapped to `(line, col)` and remapped through the
//! inline source map back to the `.vue` source, byte-for-byte matching the
//! diagnostic set the temp-file path produced (the PERF-0 Rail B parity oracle).
//!
//! The `--declaration` emit stage stays on the temp-file `tsgo --project` path
//! (tsgo `--api` exposes no emit surface) — see [`crate::checker`].

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use verter_tsgo_api::proto::types::{Diagnostic as ApiDiagnostic, UpdateSnapshotParams};
use verter_tsgo_api::snapshot::{AccessibleEntries, RealDirSource};
use verter_tsgo_api::TsgoClient;
use verter_workspace::native_fs::NativeFs;
use verter_workspace::tsgo_virtual_config::build_virtual_overlay_snapshot;

use crate::error_map::map_tsc_position;
use crate::offset_map::offset_to_line_col;
use crate::reporter::{Diagnostic, Severity};

/// How a carrier file's diagnostics map back to user-visible positions.
pub enum RemapKind {
    /// A generated validation TSX carrier: remap the converted `(line, col)`
    /// through the carrier's inline source map to its `.vue` source. When the
    /// source map has no mapping for that position, fall back to the `.vue`
    /// source at `(1, 1)` (mirrors the temp-file path's source-map fallback).
    SourceMapped { vue_path: String },
    /// A public-API stub, ambient shim, or other carrier whose OWN position is
    /// the user-visible position: keep the engine's file + the converted
    /// `(line, col)` (the stub `.vue.ts` carriers are pinned this way).
    Passthrough,
}

/// One in-memory overlay carrier fed to the engine.
pub struct OverlayFile {
    /// The virtual absolute path (forward-slashed) the carrier is served at —
    /// the overlay key AND the `files` entry the synthetic tsconfig lists.
    pub path: String,
    /// The carrier source the engine type-checks.
    pub content: String,
    /// How diagnostics on this carrier remap to user-visible positions.
    pub remap: RemapKind,
}

/// Inputs to one in-memory `--api` typecheck pass.
pub struct TypecheckInputs<'a> {
    /// The discovered native tsgo engine binary (NOT a `.cmd` shim — the `--api`
    /// pipe transport requires a native binary).
    pub engine: &'a Path,
    /// The engine working directory (the project root, for module resolution).
    pub cwd: &'a Path,
    /// The virtual synthetic-tsconfig path (overlay key + `openProject` target).
    pub tsconfig_path: String,
    /// The synthetic tsconfig JSON served at `tsconfig_path`.
    pub tsconfig_bytes: String,
    /// Every generated carrier (TSX + stubs + ambient shims).
    pub files: Vec<OverlayFile>,
}

/// Run the in-memory `--api` typecheck and return the remapped diagnostics.
/// On any engine/connect/protocol failure this returns an empty vector and logs
/// to stderr (fail-closed — never a silent tsc fallback).
pub fn typecheck(inputs: TypecheckInputs<'_>) -> Vec<Diagnostic> {
    let TypecheckInputs {
        engine,
        cwd,
        tsconfig_path,
        tsconfig_bytes,
        files,
    } = inputs;

    // Path → carrier lookup (normalized keys so the engine's reported path form
    // hits the right carrier regardless of separator / drive-letter case).
    let lookup: HashMap<String, &OverlayFile> =
        files.iter().map(|f| (norm_key(&f.path), f)).collect();

    // Carriers served through the overlay alongside the synthetic config.
    let companions: Vec<(String, String)> = files
        .iter()
        .map(|f| (f.path.clone(), f.content.clone()))
        .collect();

    let real: Arc<dyn RealDirSource> = Arc::new(FsRealDirSource::new());
    let snapshot =
        build_virtual_overlay_snapshot(&tsconfig_path, &tsconfig_bytes, &companions, real);

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("verter-tsc: failed to start async runtime for tsgo --api: {e}");
            return Vec::new();
        }
    };

    runtime.block_on(async move {
        // The wire gate runs inside `connect` (fail-closed on a diverged engine).
        let client = match TsgoClient::connect(engine, cwd, snapshot, 16) {
            Ok(c) => c,
            Err(e) => {
                eprintln!(
                    "verter-tsc: tsgo --api unavailable ({e}); no diagnostics produced \
                     (there is no tsc fallback for the typecheck path)"
                );
                return Vec::new();
            }
        };
        let diags = collect_diagnostics(&client, &tsconfig_path, &lookup).await;
        let _ = client.close().await;
        diags
    })
}

/// Drive the engine: initialize, open the configured project, then enumerate and
/// query EVERY `root_file` (TSX + stubs + ambient `.d.ts`) for semantic +
/// syntactic diagnostics.
async fn collect_diagnostics(
    client: &TsgoClient,
    tsconfig_path: &str,
    lookup: &HashMap<String, &OverlayFile>,
) -> Vec<Diagnostic> {
    if let Err(e) = client.initialize().await {
        eprintln!("verter-tsc: tsgo --api initialize failed: {e}");
        return Vec::new();
    }

    let params = UpdateSnapshotParams {
        open_project: Some(tsconfig_path.to_string()),
        file_changes: None,
    };
    let snap = match client.update_snapshot(&params).await {
        Ok(s) => s,
        Err(e) => {
            eprintln!("verter-tsc: tsgo --api updateSnapshot failed: {e}");
            return Vec::new();
        }
    };

    // Select the CONFIGURED project for our virtual tsconfig (never an inferred
    // single-file fallback) — its `root_files` is the membership oracle.
    let project = match snap
        .projects
        .iter()
        .find(|p| paths_equal(&p.config_file_name, tsconfig_path))
    {
        Some(p) => p,
        None => {
            eprintln!(
                "verter-tsc: tsgo --api did not open the configured project ({tsconfig_path})"
            );
            return Vec::new();
        }
    };

    let mut out = Vec::new();
    // Enumerate ALL configured-project root files — TSX carriers AND the
    // `.vue.ts` public-API stubs AND the ambient `.d.ts` shims — not just the
    // `.vue`-derived TSX, or stub-carrier diagnostics drop.
    for root in &project.root_files {
        match client
            .get_semantic_diagnostics(&snap.snapshot, &project.id, root)
            .await
        {
            Ok(diags) => push_mapped(&mut out, &diags, root, lookup),
            Err(e) => eprintln!("verter-tsc: tsgo --api getSemanticDiagnostics({root}): {e}"),
        }
        match client
            .get_syntactic_diagnostics(&snap.snapshot, &project.id, root)
            .await
        {
            Ok(diags) => push_mapped(&mut out, &diags, root, lookup),
            Err(e) => eprintln!("verter-tsc: tsgo --api getSyntacticDiagnostics({root}): {e}"),
        }
    }
    out
}

fn push_mapped(
    out: &mut Vec<Diagnostic>,
    diags: &[ApiDiagnostic],
    queried_root: &str,
    lookup: &HashMap<String, &OverlayFile>,
) {
    for d in diags {
        if let Some(mapped) = map_one(d, queried_root, lookup) {
            out.push(mapped);
        }
    }
}

/// Map a single `--api` diagnostic to a displayable [`Diagnostic`], or `None`
/// when it is not an error/warning, is a known Vue-JSX type gap, or its carrier
/// content is unknown.
fn map_one(
    d: &ApiDiagnostic,
    queried_root: &str,
    lookup: &HashMap<String, &OverlayFile>,
) -> Option<Diagnostic> {
    // Only error (1) + warning (0) reach tsc-style output. Suggestion (2) /
    // message (3) categories are never printed by `tsgo --project --noEmit`.
    let severity = match d.category {
        1 => Severity::Error,
        0 => Severity::Warning,
        _ => return None,
    };

    // A per-file getter reports `file_name == queried_root`; fall back to the
    // queried root if the engine omits it.
    let file_name = d.file_name.as_deref().unwrap_or(queried_root);
    let file = lookup
        .get(&norm_key(file_name))
        .or_else(|| lookup.get(&norm_key(queried_root)))?;

    // Suppress the known Vue-JSX type gaps (children / textContent / innerHTML on
    // Vue intrinsic-element attribute types) the temp-file path also suppresses
    // (tsgo preview does not honor the cross-file HTMLAttributes augmentation).
    if crate::checker::is_vue_jsx_type_gap(d.code, &d.text) {
        return None;
    }

    let (gen_line, gen_col) = offset_to_line_col(&file.content, d.pos);

    let (file_out, line_out, col_out) = match &file.remap {
        RemapKind::Passthrough => (slashed(file_name), gen_line, gen_col),
        RemapKind::SourceMapped { vue_path } => {
            match map_tsc_position(&file.content, gen_line, gen_col) {
                Some((src_name, pos)) => (
                    resolve_src_display(&src_name, vue_path),
                    pos.line + 1,
                    pos.col + 1,
                ),
                // No source-map mapping for this position: report at the .vue (1,1).
                None => (slashed(vue_path), 1, 1),
            }
        }
    };

    Some(Diagnostic {
        file: file_out,
        line: line_out,
        col: col_out,
        severity,
        ts_code: d.code,
        message: d.text.clone(),
    })
}

/// Resolve a source-map `sources[]` entry to a display path, mirroring the
/// temp-file path's `remap_diagnostics` resolution: a `file://` URL or an
/// absolute path is used as-is; a relative entry resolves against the `.vue`
/// file's directory.
fn resolve_src_display(src_name: &str, vue_path: &str) -> String {
    let display = if let Some(rest) = src_name.strip_prefix("file://") {
        rest.trim_start_matches('/').replace("%20", " ")
    } else if src_name.starts_with('/') || src_name.contains(':') {
        src_name.to_string()
    } else {
        Path::new(vue_path)
            .parent()
            .map(|p| p.join(src_name).to_string_lossy().into_owned())
            .unwrap_or_else(|| src_name.to_string())
    };
    slashed(&display)
}

/// Forward-slash a path for display / comparison.
fn slashed(p: &str) -> String {
    p.replace('\\', "/")
}

/// Lookup-map key: forward-slashed, case-folded on Windows (NTFS is
/// case-insensitive, and the engine may echo a different drive-letter case).
fn norm_key(p: &str) -> String {
    let s = slashed(p);
    if cfg!(windows) {
        s.to_ascii_lowercase()
    } else {
        s
    }
}

/// Path equality used to select the configured project: forward-slashed, and
/// case-insensitive on Windows.
fn paths_equal(a: &str, b: &str) -> bool {
    norm_key(a) == norm_key(b)
}

/// A [`RealDirSource`] backed by the sanctioned native-FS boundary
/// [`verter_workspace::native_fs::NativeFs`]. verter-tsc reads the real project +
/// `node_modules` tree from disk (the `.vue` carriers live in the host's VFS, but
/// the real dependency tree does not), so the overlay merges its virtual carriers
/// with the real directory listings (a node_modules-hiding overlay would break
/// module resolution). Routing through `NativeFs` keeps `std::fs` out of this crate
/// (the D14 / VFS-boundary architecture invariant).
#[derive(Debug)]
struct FsRealDirSource {
    fs: NativeFs,
}

impl FsRealDirSource {
    fn new() -> Self {
        Self {
            fs: NativeFs::new(),
        }
    }
}

impl RealDirSource for FsRealDirSource {
    fn real_entries(&self, dir: &str) -> Option<AccessibleEntries> {
        let entries = self.fs.read_dir(dir).ok()?;
        let mut files = Vec::new();
        let mut directories = Vec::new();
        for entry in entries {
            // `DirEntry.path` is the full canonical path; the overlay merge wants
            // the basename split into files vs directories.
            let name = entry
                .path
                .rsplit('/')
                .next()
                .unwrap_or(entry.path.as_str())
                .to_string();
            if entry.is_dir {
                directories.push(name);
            } else {
                files.push(name);
            }
        }
        Some(AccessibleEntries { files, directories })
    }
}

#[cfg(test)]
#[path = "api_check_tests.rs"]
mod tests;
