//! In-memory tsgo `--api` typecheck — the verter-tsc `--noEmit` backend.
//!
//! This replaces the temp-file `tsgo --project` subprocess path for type
//! checking. The generated carriers (validation TSX, public-API stubs, ambient
//! `.d.ts` shims) and the synthetic tsconfig are fed to the engine as an
//! in-memory [`OverlaySnapshot`] — NO temp files, NO subprocess spawn, NO poll
//! loop. The engine is the gated [`TsgoClient`] (`--api`-only); a diverged /
//! unknown engine fails the wire gate at `connect`. There is NO tsc fallback for
//! this path, and "fail-closed" means the failure is SURFACED as a hard
//! [`TypecheckError`] (→ non-zero process exit + a stderr note), NEVER swallowed
//! into an empty diagnostic set: a broken/absent engine returning empty
//! diagnostics + exit 0 would falsely advertise a clean typecheck (a broken
//! engine masquerading as "no type errors").
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
use verter_workspace::tsgo_virtual_config::{
    build_virtual_overlay_snapshot, strip_injected_root_diagnostics, InjectedPathSet,
};

use verter_span::diag_source::{
    DiagnosticContentSource, DiagnosticSourceCache, OverlayThenFallback,
};
use verter_span::path::{fs_paths_equal, InjectedPathKey};

use crate::error_map::map_tsc_position;
use crate::reporter::{Diagnostic, Severity};

/// A hard failure of the in-memory `--api` typecheck path.
///
/// The `--noEmit` typecheck backend is tsgo-`--api`-only with NO tsc fallback, so
/// an absent / wire-diverged engine or a connect/init/updateSnapshot/protocol/
/// project-not-found failure means the typecheck genuinely could not run. This
/// error is SURFACED (the caller exits non-zero + prints the message to stderr)
/// rather than swallowed into an empty diagnostic set — an empty set + exit 0
/// would falsely report a clean typecheck.
#[derive(Debug)]
pub struct TypecheckError {
    /// A user-facing explanation, printed to stderr at the process boundary.
    pub message: String,
}

impl TypecheckError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for TypecheckError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for TypecheckError {}

/// A diagnostic whose position could NOT be resolved because its source content is
/// unavailable — an infrastructure MISS, never a fabricated position.
///
/// The old path silently fell back to `(1, 1)` on a content miss, mis-homing the
/// diagnostic to the file's first character. Surfacing this as an explicit error
/// (propagated to a fatal [`TypecheckError`]) is the fail-closed contract: a
/// diagnostic is NEVER emitted at a guessed position and NEVER silently dropped.
#[derive(Debug, Clone)]
pub enum MappingError {
    /// The content for `file_name` could not be resolved (not an overlay carrier
    /// and not readable through the real-FS fallback), so the UTF-16 `pos` cannot
    /// be converted to a `(line, col)`.
    SourceUnavailable {
        /// The engine-reported file path whose content is missing.
        file_name: String,
        /// The TS diagnostic code that would have been surfaced.
        diagnostic_code: u32,
        /// The collection origin of the un-positionable diagnostic.
        origin: DiagOrigin,
    },
}

impl std::fmt::Display for MappingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MappingError::SourceUnavailable {
                file_name,
                diagnostic_code,
                origin,
            } => write!(
                f,
                "cannot position {origin:?} diagnostic TS{diagnostic_code}: source content for \
                 '{file_name}' is unavailable (not an overlay carrier and not readable from disk)"
            ),
        }
    }
}

impl std::error::Error for MappingError {}

/// A [`DiagnosticContentSource`] backed by the sanctioned native-FS boundary — the
/// real-filesystem fallback for a non-overlay file's content. Keeps `std::fs` out
/// of this crate (routes through [`NativeFs`], the VFS-boundary invariant).
struct NativeFsSource {
    fs: NativeFs,
}

impl DiagnosticContentSource for NativeFsSource {
    fn resolve(&self, raw_path: &str) -> Option<Arc<str>> {
        self.fs.read_file(raw_path)
    }
}

/// Which whole-program getter produced a diagnostic — the diagnostic's COLLECTION
/// ORIGIN. The wire [`ApiDiagnostic`] does not carry this (only `code`, `category`,
/// `text`, `pos`, `end`, `file_name`), so it is tagged at the collection boundary
/// where the three getters are called and threaded to mapping.
///
/// The origin gates the fail-closed injected-root map-boundary guard: ONLY a
/// `Config`-origin diagnostic pointing at an injected companion is suppressed;
/// `Semantic`/`Syntactic` diagnostics on a generated carrier are the LEGITIMATE
/// `.vue` remap path and are never suppressed by that guard (over-dropping them
/// would be a false negative).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagOrigin {
    /// `getSemanticDiagnosticsForProgram` — whole-program semantic errors.
    Semantic,
    /// `getSyntacticDiagnosticsForProgram` — whole-program syntactic errors.
    Syntactic,
    /// `getConfigFileParsingDiagnostics` — config-file-parse / compiler-options
    /// diagnostics. The ONLY origin whose injected-companion diagnostics are a
    /// virtualization artifact to be suppressed.
    Config,
}

/// A wire diagnostic paired with its collection origin. Borrows the wire
/// diagnostic (mapping does not mutate the shared proto struct).
pub struct OriginDiagnostic<'a> {
    pub d: &'a ApiDiagnostic,
    pub origin: DiagOrigin,
}

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
    /// The virtual synthetic-tsconfig path (overlay key + `openProjects` target).
    pub tsconfig_path: String,
    /// The synthetic tsconfig JSON served at `tsconfig_path`.
    pub tsconfig_bytes: String,
    /// Every generated carrier (TSX + stubs + ambient shims).
    pub files: Vec<OverlayFile>,
}

/// Run the in-memory `--api` typecheck and return the remapped diagnostics.
///
/// Fail-closed = SURFACE the failure: on any engine/connect/init/updateSnapshot/
/// protocol/project-not-found failure this returns [`Err`] (the caller exits
/// non-zero + prints the message). It NEVER returns an empty `Ok` for a failed
/// run — that would falsely advertise a clean typecheck. There is no tsc fallback.
pub fn typecheck(inputs: TypecheckInputs<'_>) -> Result<Vec<Diagnostic>, TypecheckError> {
    let TypecheckInputs {
        engine,
        cwd,
        tsconfig_path,
        tsconfig_bytes,
        files,
    } = inputs;

    // Path → carrier lookup, keyed by the shared filesystem-identity key
    // ([`InjectedPathKey`]) so the engine's reported path form hits the right
    // carrier regardless of separator / drive-letter case / extended-length prefix /
    // (on a case-insensitive FS) non-drive case — the SAME identity notion the
    // config strip, the map-boundary guard, and the source cache use.
    let lookup: HashMap<InjectedPathKey, &OverlayFile> = files
        .iter()
        .map(|f| (InjectedPathKey::new(&f.path), f))
        .collect();

    // Carriers served through the overlay alongside the synthetic config.
    let companions: Vec<(String, String)> = files
        .iter()
        .map(|f| (f.path.clone(), f.content.clone()))
        .collect();

    // The injected-companion paths (the generated carriers + the synthetic
    // tsconfig itself) — the set a config-parse diagnostic may reference purely as
    // a virtualization artifact. Config diagnostics pointing at these are stripped
    // (invisible to the user); real user-config errors and global (fileName:None)
    // diagnostics survive.
    let mut injected_paths: Vec<String> = files.iter().map(|f| f.path.clone()).collect();
    injected_paths.push(tsconfig_path.clone());

    let real: Arc<dyn RealDirSource> = Arc::new(FsRealDirSource::new());
    let snapshot =
        build_virtual_overlay_snapshot(&tsconfig_path, &tsconfig_bytes, &companions, real);

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            return Err(TypecheckError::new(format!(
                "verter-tsc: failed to start async runtime for tsgo --api: {e}"
            )));
        }
    };

    runtime.block_on(async move {
        // The wire gate runs inside `connect` (fail-closed on a diverged engine).
        let client = match TsgoClient::connect(engine, cwd, snapshot, 16) {
            Ok(c) => c,
            Err(e) => {
                return Err(TypecheckError::new(format!(
                    "verter-tsc: tsgo --api unavailable ({e}); the --noEmit typecheck cannot run \
                     (there is no tsc fallback for the typecheck path)"
                )));
            }
        };
        // Always close the client, then surface the collected result (Ok or Err).
        let result = collect_diagnostics(&client, &tsconfig_path, &lookup, &injected_paths).await;
        let _ = client.close().await;
        result
    })
}

/// Build the per-collection diagnostic-source cache: OVERLAY (the in-hand generated
/// carriers) FIRST, then the real filesystem via the `NativeFs` boundary. Every
/// diagnostic's UTF-16 offset is positioned through this cache, so each file's
/// content is resolved ONCE and its line index built ONCE per pass (never per
/// diagnostic).
fn build_source_cache(
    lookup: &HashMap<InjectedPathKey, &OverlayFile>,
) -> DiagnosticSourceCache<OverlayThenFallback<NativeFsSource>> {
    let overlay = lookup.values().map(|f| (f.path.clone(), f.content.clone()));
    let source = OverlayThenFallback::new(
        overlay,
        NativeFsSource {
            fs: NativeFs::new(),
        },
    );
    DiagnosticSourceCache::new(source)
}

/// Drive the engine: initialize, open the configured project, then collect the
/// WHOLE-PROGRAM diagnostics — one file-omitted semantic call + one file-omitted
/// syntactic call (which cover every program file, INCLUDING non-root imported
/// `.ts` the old per-root loop never queried) PLUS the config-file-parsing
/// diagnostics (options/global, not covered by the per-file getters). This is
/// whole-program parity with the old `tsgo --project --noEmit` path.
///
/// Fail-closed: any protocol failure on any call is a HARD error (never an
/// eprintln-and-continue) — silently dropping diagnostics is exactly the
/// quiet-failure this path bans.
async fn collect_diagnostics(
    client: &TsgoClient,
    tsconfig_path: &str,
    lookup: &HashMap<InjectedPathKey, &OverlayFile>,
    injected_paths: &[String],
) -> Result<Vec<Diagnostic>, TypecheckError> {
    if let Err(e) = client.initialize().await {
        return Err(TypecheckError::new(format!(
            "verter-tsc: tsgo --api initialize failed: {e}"
        )));
    }

    // OWNED one-shot: connect → 1×updateSnapshot → diagnostics → drop. A single
    // `open_projects: [tsconfig]` is correct (the client is torn down after, so
    // no lease bookkeeping is required).
    let params = UpdateSnapshotParams::single_project(tsconfig_path.to_string());
    let snap = match client.update_snapshot(&params).await {
        Ok(s) => s,
        Err(e) => {
            return Err(TypecheckError::new(format!(
                "verter-tsc: tsgo --api updateSnapshot failed: {e}"
            )));
        }
    };

    // Select the CONFIGURED project for our virtual tsconfig (never an inferred
    // single-file fallback).
    let project = match snap
        .projects
        .iter()
        .find(|p| fs_paths_equal(&p.config_file_name, tsconfig_path))
    {
        Some(p) => p,
        None => {
            return Err(TypecheckError::new(format!(
                "verter-tsc: tsgo --api did not open the configured project ({tsconfig_path})"
            )));
        }
    };

    // ONE whole-program semantic call + ONE whole-program syntactic call: the
    // `file` argument is omitted, so the engine returns diagnostics for EVERY file
    // in the program (root carriers AND non-root imported sources).
    let semantic = client
        .get_semantic_diagnostics_for_program(&snap.snapshot, &project.id)
        .await
        .map_err(|e| {
            TypecheckError::new(format!(
                "verter-tsc: tsgo --api getSemanticDiagnostics (whole program): {e}"
            ))
        })?;
    let syntactic = client
        .get_syntactic_diagnostics_for_program(&snap.snapshot, &project.id)
        .await
        .map_err(|e| {
            TypecheckError::new(format!(
                "verter-tsc: tsgo --api getSyntacticDiagnostics (whole program): {e}"
            ))
        })?;

    // Config-file parse / compiler-options diagnostics (options/global — NOT
    // covered by the two per-file getters). Strip only the diagnostics that point
    // at an injected companion (a virtualization artifact); real user-config
    // errors and global (fileName:None) diagnostics survive (fail-closed: only
    // KNOWN injected companions are discarded).
    let config = client
        .get_config_file_parsing_diagnostics(&snap.snapshot, &project.id)
        .await
        .map_err(|e| {
            TypecheckError::new(format!(
                "verter-tsc: tsgo --api getConfigFileParsingDiagnostics: {e}"
            ))
        })?;

    // ONE injected-companion set, shared by the upstream config strip AND the
    // fail-closed map-boundary guard, so the strip decision and the diagnostic-
    // homing decision share a single filesystem-identity notion (never a second
    // ad-hoc path key that could diverge on separator / drive-letter case).
    let injected_set = InjectedPathSet::from_paths(injected_paths);
    let config = strip_injected_root_diagnostics(config, &injected_set);

    // The per-collection source cache: OVERLAY carriers first, then the real FS.
    // Every offset is positioned through it, so each file resolves + indexes ONCE.
    let cache = build_source_cache(lookup);

    let mut out = Vec::new();
    push_mapped(
        &mut out,
        &semantic,
        DiagOrigin::Semantic,
        lookup,
        &cache,
        &injected_set,
    )?;
    push_mapped(
        &mut out,
        &syntactic,
        DiagOrigin::Syntactic,
        lookup,
        &cache,
        &injected_set,
    )?;
    push_mapped(
        &mut out,
        &config,
        DiagOrigin::Config,
        lookup,
        &cache,
        &injected_set,
    )?;
    Ok(out)
}

/// Map a diagnostic stream, pushing every surfaced diagnostic. A content/source
/// MISS on any diagnostic is FATAL: it propagates as a [`TypecheckError`] (non-zero
/// exit + stderr note) rather than a fabricated position or a silent drop.
fn push_mapped(
    out: &mut Vec<Diagnostic>,
    diags: &[ApiDiagnostic],
    origin: DiagOrigin,
    lookup: &HashMap<InjectedPathKey, &OverlayFile>,
    cache: &DiagnosticSourceCache<OverlayThenFallback<NativeFsSource>>,
    injected: &InjectedPathSet,
) -> Result<(), TypecheckError> {
    for d in diags {
        let od = OriginDiagnostic { d, origin };
        match map_one(&od, lookup, cache, injected) {
            Ok(Some(mapped)) => out.push(mapped),
            // `Ok(None)` = not an error/warning, a suppressed Vue-JSX gap, or a
            // suppressed injected-companion config diagnostic (not a failure).
            Ok(None) => {}
            // A genuine content/source miss: surface it, never guess a position.
            Err(e) => {
                return Err(TypecheckError::new(format!(
                    "verter-tsc: {e}; the typecheck cannot position this diagnostic (fail-closed: \
                     no fabricated position, no silent drop)"
                )));
            }
        }
    }
    Ok(())
}

/// Map a single `--api` diagnostic to a displayable [`Diagnostic`], or `None`
/// when it is not an error/warning or is a known Vue-JSX type gap.
///
/// WHOLE-PROGRAM ATTRIBUTION (fail-closed, never mis-mapped): every diagnostic is
/// homed by its OWN `file_name`, exactly as the old whole-program `tsgo --project`
/// path did. Three cases:
///
///   1. `file_name` resolves to a KNOWN overlay carrier (a generated TSX / `.vue.ts`
///      stub / ambient shim). Carrier classification folds the engine-reported path
///      onto the registered carrier through the shared filesystem-identity key
///      ([`InjectedPathKey`]) — the SAME notion the config strip, the map-boundary
///      guard, and the source cache use — so a separator / drive-letter-case /
///      extended-length-prefix / (case-insensitive-FS) non-drive-case variant of the
///      carrier path still hits it. Map through that carrier: a `SourceMapped`
///      carrier remaps its UTF-16 → (line,col) through the inline source map back to
///      the `.vue` source; a `Passthrough` carrier keeps its own converted position.
///   2. `file_name` is PRESENT but is NOT a carrier (a real non-root imported
///      `.ts` the whole-program call surfaces): surface it as a PASSTHROUGH at its
///      OWN path + converted position, reading the file's content through the shared
///      source cache (overlay-then-disk) to position the UTF-16 offset. A non-root
///      diagnostic is NEVER dropped and NEVER re-homed onto a carrier (that would
///      remap through the wrong source map). When the content genuinely cannot be
///      resolved, this returns an EXPLICIT [`MappingError`] (→ a fatal
///      `TypecheckError`) — never a fabricated position, never a silent drop.
///   3. `file_name` is ABSENT (a global / compiler-options diagnostic, e.g. a bad
///      `target`): surface it at the project's own position `(1,1)` under a
///      synthetic `<compiler options>` label — a global diagnostic is RETAINED,
///      not dropped. This is NOT a content miss (there is no file to resolve); it is
///      distinct from the case-1 source-map GAP, which reports a RESOLVED carrier's
///      un-tokenized position at the `.vue` `(1,1)`.
///
/// Injected-companion CONFIG noise is filtered UPSTREAM by
/// `strip_injected_root_diagnostics`. A fail-closed BELT-AND-SUSPENDERS guard here
/// re-checks the SAME injected set at the map boundary (before any carrier remap):
/// a `Config`-origin diagnostic pointing at an injected companion is suppressed. The
/// reopen proved the upstream strip alone leaks when the engine echoes a companion
/// under a different drive-letter case / separator; the shared filesystem-identity
/// key closes that. The guard fires ONLY for `Config` origin — a
/// `Semantic`/`Syntactic` diagnostic on a generated carrier is the LEGITIMATE `.vue`
/// remap path and is never suppressed (over-dropping = false negatives).
fn map_one(
    od: &OriginDiagnostic<'_>,
    lookup: &HashMap<InjectedPathKey, &OverlayFile>,
    cache: &DiagnosticSourceCache<OverlayThenFallback<NativeFsSource>>,
    injected: &InjectedPathSet,
) -> Result<Option<Diagnostic>, MappingError> {
    let d = od.d;

    // FAIL-CLOSED map-boundary invariant (keyed by the shared filesystem-identity
    // key AND origin), independent of the upstream config strip: a
    // Config/injected-root-origin diagnostic pointing at an injected companion is a
    // virtualization artifact and must never be emitted or re-homed onto `.vue`.
    // Non-Config origins are NOT touched here — they take the legitimate carrier
    // remap below.
    if od.origin == DiagOrigin::Config {
        if let Some(file_name) = d.file_name.as_deref() {
            if injected.contains(file_name) {
                return Ok(None);
            }
        }
    }

    // Only error (1) + warning (0) reach tsc-style output. Suggestion (2) /
    // message (3) categories are never printed by `tsgo --project --noEmit`.
    let severity = match d.category {
        1 => Severity::Error,
        0 => Severity::Warning,
        _ => return Ok(None),
    };

    // Suppress the known Vue-JSX type gaps (children / textContent / innerHTML on
    // Vue intrinsic-element attribute types) the temp-file path also suppresses
    // (tsgo preview does not honor the cross-file HTMLAttributes augmentation).
    if crate::checker::is_vue_jsx_type_gap(d.code, &d.text) {
        return Ok(None);
    }

    let (file_out, line_out, col_out) = match d.file_name.as_deref() {
        // Case 3: a global / compiler-options diagnostic (no file). Surface it at
        // a synthetic position rather than dropping it (this is NOT a content miss —
        // there is no file to resolve).
        None => ("<compiler options>".to_string(), 1, 1),
        Some(file_name) => match lookup.get(&InjectedPathKey::new(file_name)) {
            // Case 1: a known overlay carrier. Its content is in the overlay, so the
            // cache resolves it (a miss here is an internal inconsistency, surfaced).
            Some(file) => {
                let (gen_line, gen_col) = line_col_via_cache(cache, &file.path, od)?;
                match &file.remap {
                    RemapKind::Passthrough => (slashed(file_name), gen_line, gen_col),
                    RemapKind::SourceMapped { vue_path } => {
                        match map_tsc_position(&file.content, gen_line, gen_col) {
                            Some((src_name, pos)) => (
                                resolve_src_display(&src_name, vue_path),
                                pos.line + 1,
                                pos.col + 1,
                            ),
                            // No source-map TOKEN for this position (the carrier
                            // content DID resolve): report at the .vue (1,1). This is
                            // a source-map gap, not a content miss.
                            None => (slashed(vue_path), 1, 1),
                        }
                    }
                }
            }
            // Case 2: a real non-root file (imported `.ts`, etc.). Position it by its
            // OWN content, resolved through the shared cache (overlay-then-disk). A
            // genuine content miss is an EXPLICIT error — never a fabricated (1,1),
            // never a silent drop.
            None => {
                let (line, col) = line_col_via_cache(cache, file_name, od)?;
                (slashed(file_name), line, col)
            }
        },
    };

    Ok(Some(Diagnostic {
        file: file_out,
        line: line_out,
        col: col_out,
        severity,
        ts_code: d.code,
        message: d.text.clone(),
    }))
}

/// Convert the diagnostic's UTF-16 `pos` to a 1-based `(line, col)` through the
/// per-collection source cache's build-once line index for `content_path`. A
/// content/source MISS is an explicit [`MappingError::SourceUnavailable`] — the
/// fail-closed contract (no fabricated position).
fn line_col_via_cache(
    cache: &DiagnosticSourceCache<OverlayThenFallback<NativeFsSource>>,
    content_path: &str,
    od: &OriginDiagnostic<'_>,
) -> Result<(u32, u32), MappingError> {
    let Some(sf) = cache.source_file(content_path) else {
        return Err(MappingError::SourceUnavailable {
            file_name: content_path.to_string(),
            diagnostic_code: od.d.code,
            origin: od.origin,
        });
    };
    let lc = sf
        .line_index()
        .line_col_for_utf16(od.d.pos)
        .expect("Utf16LineIndex built by new() always has line starts");
    Ok((lc.line, lc.col))
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
