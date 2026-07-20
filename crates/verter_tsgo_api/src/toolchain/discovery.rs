//! The ordered first-working tsgo resolver.
//!
//! Resolution walks four tiers IN ORDER and selects the first candidate that
//! validates ([`crate::toolchain::validation`]); a candidate that fails
//! validation is recorded as an actionable rejection and the walk continues:
//!
//! 1. **shared** — `VERTER_TSGO_BIN` (when it names an existing file), then
//!    `tsc[.exe]`/`tsgo[.exe]` found by direct `PATH` traversal;
//! 2. **project-local** — from the bound project root upward through ancestor
//!    `node_modules`: the exact host platform package (flat and pnpm store
//!    layouts), then the `.bin` shim — NEVER a foreign-platform package;
//! 3. **temp cache** — existing `verter-tsgo-v1/<user>/<triple>/<policy>/<v>/`
//!    entries, newest supported version first (consume-only here; the online
//!    downloader is what writes them). The cache tree is trust-checked: no
//!    symlink/reparse-point components, owner-only root on Unix;
//! 4. **bundled** — the sidecar at `<host-exe-dir>/tsgo/lib/tsc[.exe]`
//!    (location contract in [`crate::toolchain::bundle`]; the packaged product
//!    ships the binary). A bundled candidate that EXISTS but fails validation
//!    — or is structurally invalid (a symlink/reparse component) — is a
//!    PRODUCT-INTEGRITY failure, not a "no provider" outcome.
//!
//! Resolution NEVER touches the network.

use std::collections::HashSet;
use std::fmt;
use std::path::{Path, PathBuf};

use super::bundle::bundled_tsgo_path;
use super::platform::{host_platform, TsgoPlatform};
use super::policy::{TsgoVersion, VersionPolicy, SUPPORTED_POLICY_ID};
use super::validation::{CandidateValidator, Capability, ProcessValidator, RejectionReason};

/// The explicit engine-override environment variable (tier 1).
pub const ENV_OVERRIDE_VAR: &str = "VERTER_TSGO_BIN";

/// The temp-cache root directory name under the system temp dir.
const CACHE_DIR_NAME: &str = "verter-tsgo-v1";

/// The marker file a complete cache entry carries (written last by the
/// downloader; a directory without it is an incomplete install).
const READY_MARKER: &str = "READY.json";

/// Which tier a candidate came from (its provenance).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provenance {
    /// `VERTER_TSGO_BIN`.
    EnvOverride,
    /// A `tsc`/`tsgo` found by `PATH` traversal.
    SharedPath,
    /// A project-local `node_modules` install.
    ProjectLocal,
    /// A supported version in the temp update cache.
    TempCache,
    /// The bundled sidecar next to the running executable.
    Bundled,
}

impl fmt::Display for Provenance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EnvOverride => write!(f, "VERTER_TSGO_BIN override"),
            Self::SharedPath => write!(f, "shared PATH install"),
            Self::ProjectLocal => write!(f, "project-local node_modules"),
            Self::TempCache => write!(f, "update cache"),
            Self::Bundled => write!(f, "bundled sidecar"),
        }
    }
}

/// One discovered engine binary, before validation.
#[derive(Debug, Clone)]
pub struct Candidate {
    /// The engine binary path.
    pub path: PathBuf,
    /// The tier that contributed it.
    pub provenance: Provenance,
}

/// A candidate that failed validation, with its provenance — one line of the
/// resolver's actionable rejection report.
#[derive(Debug, Clone)]
pub struct CandidateRejection {
    /// The rejected candidate.
    pub candidate: Candidate,
    /// Why it failed validation.
    pub reason: RejectionReason,
}

impl fmt::Display for CandidateRejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "[{}] {}: {}",
            self.candidate.provenance,
            self.candidate.path.display(),
            self.reason
        )
    }
}

/// What a resolution attempt searches. Construct with
/// [`ResolutionRequest::for_environment`] in production; tests build the
/// fields directly so no real environment leaks in.
#[derive(Debug, Clone)]
pub struct ResolutionRequest {
    /// The engine surface the winner must prove.
    pub requirement: Capability,
    /// The bound project/config root — tier 2 walks its ancestors'
    /// `node_modules`, never merely the process CWD.
    pub project_root: Option<PathBuf>,
    /// The `VERTER_TSGO_BIN` override value, if set.
    pub env_override: Option<PathBuf>,
    /// The `PATH` entries to traverse for tier 1.
    pub path_entries: Vec<PathBuf>,
    /// The temp-cache root (the system temp dir in production); `verter-tsgo-v1`
    /// is appended inside it. `None` disables tier 3.
    pub cache_root: Option<PathBuf>,
    /// The running executable (`std::env::current_exe` in production) — the
    /// bundled sidecar derives from it. `None` disables tier 4.
    pub host_exe: Option<PathBuf>,
}

impl ResolutionRequest {
    /// Build a request from the real process environment.
    pub fn for_environment(requirement: Capability, project_root: Option<PathBuf>) -> Self {
        Self {
            requirement,
            project_root,
            env_override: std::env::var_os(ENV_OVERRIDE_VAR)
                .filter(|v| !v.is_empty())
                .map(PathBuf::from),
            path_entries: std::env::var_os("PATH")
                .map(|p| std::env::split_paths(&p).collect())
                .unwrap_or_default(),
            cache_root: Some(std::env::temp_dir()),
            host_exe: std::env::current_exe().ok(),
        }
    }
}

/// The tier-ordered candidates plus notes about skipped/degraded tiers.
#[derive(Debug, Default)]
pub struct CandidateEnumeration {
    /// Candidates in tier order (shared → local → cache → bundled),
    /// deduplicated by canonical path. A canonical path reachable as the
    /// bundled sidecar ALWAYS retains [`Provenance::Bundled`] (even when an
    /// earlier tier named the same file), so integrity escalation still fires.
    pub candidates: Vec<Candidate>,
    /// The bundled sidecar EXISTS but is structurally invalid (a
    /// symlink/reparse component): a product-integrity failure, recorded here
    /// instead of degrading to a silent skip that falls through to
    /// "no provider". `None` when there is no bundled sidecar (fine — e.g. a
    /// source checkout) or it is structurally sound.
    pub invalid_bundled: Option<Box<CandidateRejection>>,
    /// Human-readable notes: stale overrides, trust-skipped tiers, unsupported
    /// hosts. Surfaced in failure diagnostics.
    pub notes: Vec<String>,
}

/// A successful resolution: the first WORKING candidate plus the rejections
/// recorded before it (for diagnostics when a later tier wins).
#[derive(Debug)]
pub struct Resolution {
    /// The validated engine binary path.
    pub path: PathBuf,
    /// Its probed, policy-accepted version.
    pub version: TsgoVersion,
    /// The tier that supplied it.
    pub provenance: Provenance,
    /// The candidates that failed validation before this one (actionable).
    pub rejections: Vec<CandidateRejection>,
}

/// Why a resolution failed.
#[derive(Debug)]
pub enum ResolveError {
    /// No candidate validated, and no bundled sidecar exists (e.g. a source
    /// checkout without the packaged sidecar). The report lists every
    /// rejection and tier note.
    NoUsableCandidate {
        /// Every candidate that failed validation.
        rejections: Vec<CandidateRejection>,
        /// Tier notes (skipped/degraded tiers).
        notes: Vec<String>,
        /// The capability the caller required.
        requirement: Capability,
    },
    /// The bundled sidecar EXISTS but failed validation: the installed
    /// product is corrupt. "No TypeProvider" is not an acceptable
    /// installed-product state — this is fatal, with a reinstall directive.
    ProductIntegrity {
        /// The bundled sidecar path.
        path: PathBuf,
        /// Why it failed validation (boxed to keep the error type small).
        reason: Box<RejectionReason>,
    },
}

impl fmt::Display for ResolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoUsableCandidate {
                rejections,
                notes,
                requirement,
            } => {
                writeln!(
                    f,
                    "no usable tsgo engine found for the `{requirement}` surface."
                )?;
                writeln!(
                    f,
                    "Verter supports tsgo (TypeScript 7 native) stable {}.",
                    super::policy::SUPPORTED_TSGO_RANGE_LABEL
                )?;
                writeln!(
                    f,
                    "searched, in order: {ENV_OVERRIDE_VAR}, PATH, project-local node_modules, \
                     the update cache, the bundled sidecar"
                )?;
                for note in notes {
                    writeln!(f, "note: {note}")?;
                }
                for rejection in rejections {
                    writeln!(f, "  - {rejection}")?;
                }
                write!(
                    f,
                    "install a supported engine (e.g. `npm install -D typescript@{}`) or point \
                     {ENV_OVERRIDE_VAR} at one",
                    super::policy::BUNDLED_TSGO_VERSION
                )
            }
            Self::ProductIntegrity { path, reason } => write!(
                f,
                "the bundled tsgo sidecar at {} failed validation — the installed Verter \
                 product is corrupt (product integrity): {reason}. Reinstall Verter; the \
                 bundled engine is the offline floor and must always work.",
                path.display()
            ),
        }
    }
}

impl std::error::Error for ResolveError {}

/// Resolve the first WORKING tsgo engine for `request` with the production
/// validator (real process probes + capability smokes).
pub async fn resolve(request: &ResolutionRequest) -> Result<Resolution, ResolveError> {
    resolve_with(request, &ProcessValidator::from_env()).await
}

/// Resolve with an explicit validator (the seam ordering tests drive).
pub async fn resolve_with(
    request: &ResolutionRequest,
    validator: &dyn CandidateValidator,
) -> Result<Resolution, ResolveError> {
    let enumeration = enumerate_candidates(request);
    let mut rejections = Vec::new();
    for candidate in enumeration.candidates {
        match validator
            .validate(&candidate.path, request.requirement)
            .await
        {
            Ok(validated) => {
                return Ok(Resolution {
                    path: validated.path,
                    version: validated.version,
                    provenance: candidate.provenance,
                    rejections,
                });
            }
            Err(reason) => {
                if candidate.provenance == Provenance::Bundled {
                    return Err(ResolveError::ProductIntegrity {
                        path: candidate.path,
                        reason: Box::new(reason),
                    });
                }
                rejections.push(CandidateRejection { candidate, reason });
            }
        }
    }
    // The walk reached the end without a working candidate. A present-but-
    // structurally-invalid bundled sidecar (the offline floor) is a
    // PRODUCT-INTEGRITY failure — loud reinstall signal, never a soft
    // no-provider miss.
    if let Some(invalid) = enumeration.invalid_bundled {
        return Err(ResolveError::ProductIntegrity {
            path: invalid.candidate.path,
            reason: Box::new(invalid.reason),
        });
    }
    Err(ResolveError::NoUsableCandidate {
        rejections,
        notes: enumeration.notes,
        requirement: request.requirement,
    })
}

/// Blocking form of [`resolve`] for SYNC callers outside any async runtime
/// context (it builds a private current-thread runtime). Panics if called from
/// within a tokio runtime context — call [`resolve`] instead there.
///
/// This is the ONLY resolution path: there is deliberately no version-only
/// variant. A candidate that merely passes `--version` + policy can mask a
/// working candidate behind a broken one, so every caller — production or
/// test — resolves through the same first-working, capability-VALIDATED walk.
pub fn resolve_blocking(request: &ResolutionRequest) -> Result<Resolution, ResolveError> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| ResolveError::NoUsableCandidate {
            rejections: Vec::new(),
            notes: vec![format!("failed to start the tsgo resolution runtime: {e}")],
            requirement: request.requirement,
        })?;
    runtime.block_on(resolve(request))
}

/// Enumerate the tier-ordered candidates for `request` (pure discovery — no
/// validation, no processes).
pub fn enumerate_candidates(request: &ResolutionRequest) -> CandidateEnumeration {
    let mut out = CandidateEnumeration::default();
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let Some(platform) = host_platform() else {
        out.notes.push(
            "unsupported host platform: no tsgo platform package mapping exists for this \
             OS/architecture"
                .to_string(),
        );
        return out;
    };

    // ── Tier 1: shared (env override, then PATH traversal) ────────────────
    if let Some(env) = &request.env_override {
        if env.is_file() {
            push(&mut out, &mut seen, env.clone(), Provenance::EnvOverride);
        } else {
            out.notes.push(format!(
                "{ENV_OVERRIDE_VAR} points at {} which is not a usable file; ignoring it",
                env.display()
            ));
        }
    }
    {
        let mut dirs: HashSet<&Path> = HashSet::new();
        for dir in &request.path_entries {
            if dir.as_os_str().is_empty() || !dirs.insert(dir.as_path()) {
                continue;
            }
            for name in platform.path_executable_names() {
                let candidate = dir.join(name);
                if candidate.is_file() {
                    push(&mut out, &mut seen, candidate, Provenance::SharedPath);
                }
            }
        }
    }

    // ── Tier 2: project-local (bound root's ancestor node_modules) ─────────
    if let Some(root) = &request.project_root {
        for ancestor in root.ancestors() {
            let node_modules = ancestor.join("node_modules");
            if !node_modules.is_dir() {
                continue;
            }
            // (a) The exact host platform package, flat layout.
            let flat = node_modules
                .join(platform.package_rel_path())
                .join(platform.lib_executable_rel_path());
            if flat.is_file() {
                push(&mut out, &mut seen, flat, Provenance::ProjectLocal);
            }
            // (b) The pnpm store layout, newest supported entry first.
            let pnpm_root = node_modules.join(".pnpm");
            if let Ok(entries) = std::fs::read_dir(&pnpm_root) {
                let prefix = format!("{}@", platform.pnpm_store_entry);
                let mut store_entries: Vec<(Option<TsgoVersion>, PathBuf)> = entries
                    .flatten()
                    .filter(|e| e.file_name().to_string_lossy().starts_with(prefix.as_str()))
                    .map(|e| {
                        let version = e
                            .file_name()
                            .to_string_lossy()
                            .strip_prefix(prefix.as_str())
                            .and_then(|v| TsgoVersion::parse(v).ok());
                        (version, e.path())
                    })
                    .collect();
                store_entries.sort_by(|a, b| b.0.cmp(&a.0));
                for (_, entry) in store_entries {
                    let bin = entry
                        .join("node_modules")
                        .join(platform.package_rel_path())
                        .join(platform.lib_executable_rel_path());
                    if bin.is_file() {
                        push(&mut out, &mut seen, bin, Provenance::ProjectLocal);
                    }
                }
            }
            // (c) The platform package nested under the `typescript` package.
            let nested = node_modules
                .join("typescript")
                .join("node_modules")
                .join(platform.package_rel_path())
                .join(platform.lib_executable_rel_path());
            if nested.is_file() {
                push(&mut out, &mut seen, nested, Provenance::ProjectLocal);
            }
            // (d) The `.bin` shims (native first, then the legacy name).
            for shim in [platform.bin_shim, platform.legacy_bin_shim()] {
                let candidate = node_modules.join(".bin").join(shim);
                if candidate.is_file() {
                    push(&mut out, &mut seen, candidate, Provenance::ProjectLocal);
                }
            }
        }
    }

    // ── Tier 3: the temp update cache (consume-only) ────────────────────
    if let Some(cache_root) = &request.cache_root {
        enumerate_cache_tier(cache_root, platform, &mut out, &mut seen);
    }

    // ── Tier 4: the bundled sidecar ────────────────────────────────────────
    if let Some(exe) = &request.host_exe {
        if let Some(bundled) = bundled_tsgo_path(exe) {
            if bundled.exists() {
                let trusted_root = exe.parent().map(Path::to_path_buf).unwrap_or_default();
                if has_symlink_components(&bundled, &trusted_root) {
                    // The sidecar EXISTS but is structurally invalid: this is a
                    // PRODUCT-INTEGRITY failure (a tampered/corrupt install),
                    // never a soft skip that falls through to "no provider".
                    out.invalid_bundled = Some(Box::new(CandidateRejection {
                        candidate: Candidate {
                            path: bundled.clone(),
                            provenance: Provenance::Bundled,
                        },
                        reason: RejectionReason::UntrustedLocation {
                            detail: format!(
                                "the bundled tsgo sidecar at {} has a symlink/reparse-point \
                                 component inside the install directory; refusing to trust it",
                                bundled.display()
                            ),
                        },
                    }));
                } else {
                    push(&mut out, &mut seen, bundled, Provenance::Bundled);
                }
            }
        }
    }

    out
}

/// The per-user cache identity (`verter-tsgo-v1/<user>/…`).
fn user_cache_id() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "shared".to_string())
}

/// Tier 3: enumerate existing cache entries, newest supported version first.
/// Consume-only: no mutation, no network. A corrupt newest entry is simply
/// skipped by validation and the walk tries older supported entries.
///
/// Trust model (execution outside the trusted tree is never allowed):
/// - the cache root itself must be OWNER-OWNED and not group/world-writable
///   (Unix: a foreign-owned or writable root lets another user swap engines);
/// - the FULL resolved path to each binary — every component through
///   `package/lib/<binary>` — must be free of symlink/reparse components;
/// - and the canonicalized binary must stay INSIDE the canonicalized cache
///   tree (belt-and-suspenders against any escape the component walk missed).
fn enumerate_cache_tier(
    cache_root: &Path,
    platform: &TsgoPlatform,
    out: &mut CandidateEnumeration,
    seen: &mut HashSet<PathBuf>,
) {
    let v1_root = cache_root.join(CACHE_DIR_NAME);
    if !v1_root.exists() {
        return; // no cache yet — nothing to say
    }
    // Trust: owner + write bits on the cache root (Unix).
    #[cfg(unix)]
    {
        if let Some(issue) = unix_cache_root_trust_issues(&v1_root, current_euid()) {
            out.notes.push(issue);
            return;
        }
    }
    let base = v1_root
        .join(user_cache_id())
        .join(platform.target_triple)
        .join(SUPPORTED_POLICY_ID);
    // Trust: no symlink/reparse-point components below the ambient temp root.
    if has_symlink_components(&base, cache_root) {
        out.notes.push(format!(
            "the tsgo update cache at {} contains a symlink/reparse-point component; \
             skipping the cache tier",
            v1_root.display()
        ));
        return;
    }
    // The canonical anchor for the per-binary in-tree assertion.
    let canonical_base = match base.canonicalize() {
        Ok(base) => base,
        Err(_) => return,
    };
    let Ok(entries) = std::fs::read_dir(&base) else {
        return;
    };
    // Cache entries are always stable releases — the production policy
    // filters, never the dev override.
    let policy = VersionPolicy::production();
    let mut versions: Vec<(TsgoVersion, PathBuf)> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let version = policy.check_str(&name).ok()?;
            let dir = entry.path();
            let binary = dir.join("package").join(platform.lib_executable_rel_path());
            // Trust: the FULL resolved path to the binary (version dir AND
            // package/lib/<binary>) must be reparse-free and canonicalize
            // INSIDE the trusted cache tree.
            if has_symlink_components(&binary, cache_root) {
                return None;
            }
            let canonical_binary = binary.canonicalize().ok()?;
            if !canonical_binary.starts_with(&canonical_base) {
                return None;
            }
            if !binary.is_file() || !dir.join(READY_MARKER).is_file() {
                return None;
            }
            Some((version, binary))
        })
        .collect();
    versions.sort_by(|a, b| b.0.cmp(&a.0));
    for (_, binary) in versions {
        push(out, seen, binary, Provenance::TempCache);
    }
}

/// The current effective uid (Unix trust check).
#[cfg(unix)]
fn current_euid() -> u32 {
    unsafe { libc::geteuid() }
}

/// Unix trust verdict on the cache root: `Some(issue)` when the root is NOT
/// owned by `euid` or is group/world-writable (another user could swap
/// engines); `None` when it is owned and private enough to trust.
/// World-READABLE is fine.
#[cfg(unix)]
fn unix_cache_root_trust_issues(v1_root: &Path, euid: u32) -> Option<String> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let metadata = std::fs::metadata(v1_root).ok()?;
    if metadata.uid() != euid {
        return Some(format!(
            "the tsgo update cache at {} is owned by uid {} but the current effective \
             uid is {euid}; skipping the cache tier",
            v1_root.display(),
            metadata.uid()
        ));
    }
    if metadata.permissions().mode() & 0o022 != 0 {
        return Some(format!(
            "the tsgo update cache at {} is group/world-writable; skipping the \
             cache tier",
            v1_root.display()
        ));
    }
    None
}

/// Whether any component of `path` below `trusted_prefix` (exclusive) is a
/// symlink/reparse point. Non-existent components are ignored (existence is
/// checked separately).
fn has_symlink_components(path: &Path, trusted_prefix: &Path) -> bool {
    let mut current = Some(path);
    while let Some(p) = current {
        if p == trusted_prefix || !p.starts_with(trusted_prefix) {
            break;
        }
        if let Ok(metadata) = std::fs::symlink_metadata(p) {
            if is_reparse_point(&metadata) {
                return true;
            }
        }
        current = p.parent();
    }
    false
}

/// Windows: the complete reparse-point check — `FILE_ATTRIBUTE_REPARSE_POINT`
/// covers directory junctions and mount points, which `FileType::is_symlink()`
/// alone misses.
#[cfg(windows)]
fn is_reparse_point(metadata: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

/// Unix/other: symlinks are the reparse-point class.
#[cfg(not(windows))]
fn is_reparse_point(metadata: &std::fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

/// Push a candidate, deduplicated by canonical path. The FIRST tier to name a
/// file keeps its slot (tier order), with ONE provenance rule on top: a
/// canonical path reachable as the bundled sidecar ALWAYS carries
/// [`Provenance::Bundled`] — the offline floor's product-integrity escalation
/// must still fire when PATH or `VERTER_TSGO_BIN` happens to point at the
/// bundled binary (never discarded by dedup).
fn push(
    out: &mut CandidateEnumeration,
    seen: &mut HashSet<PathBuf>,
    path: PathBuf,
    provenance: Provenance,
) {
    let key = path.canonicalize().unwrap_or_else(|_| path.clone());
    if seen.insert(key.clone()) {
        out.candidates.push(Candidate { path, provenance });
        return;
    }
    if provenance == Provenance::Bundled {
        if let Some(existing) = out.candidates.iter_mut().find(|c| {
            c.provenance != Provenance::Bundled
                && c.path.canonicalize().unwrap_or_else(|_| c.path.clone()) == key
        }) {
            existing.provenance = Provenance::Bundled;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::toolchain::platform::host_platform;
    use crate::toolchain::policy::{TsgoVersion, VersionPolicy};
    use crate::toolchain::validation::{Capability, RejectionReason, ValidatedCandidate};
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;

    // ── fixture helpers ─────────────────────────────────────────────────────

    /// A unique temp fixture root, removed on drop.
    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        fn new(tag: &str) -> Self {
            static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let root = std::env::temp_dir().join(format!(
                "verter-tsgo-discovery-test-{}-{}-{tag}",
                std::process::id(),
                COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(&root).unwrap();
            Self { root }
        }

        /// Materialize a file (dummy content) at `rel` under the root.
        fn file(&self, rel: &Path) -> PathBuf {
            let path = self.root.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, b"fixture").unwrap();
            path
        }

        fn path(&self, rel: &str) -> PathBuf {
            self.root.join(rel)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    /// The fixture's relative path for the host platform package binary.
    fn host_package_rel() -> PathBuf {
        let host = host_platform().unwrap();
        Path::new("@typescript")
            .join(format!("typescript-{}-{}", host.npm_os, host.npm_arch))
            .join("lib")
            .join(host.executable)
    }

    /// Build a request over explicit roots (no real environment leaks in).
    fn request(
        project_root: Option<PathBuf>,
        env_override: Option<PathBuf>,
        path_entries: Vec<PathBuf>,
        cache_root: Option<PathBuf>,
        host_exe: Option<PathBuf>,
    ) -> ResolutionRequest {
        ResolutionRequest {
            requirement: Capability::Lsp,
            project_root,
            env_override,
            path_entries,
            cache_root,
            host_exe,
        }
    }

    /// Materialize a full multi-tier fixture: env override, PATH dir, project
    /// and ancestor node_modules, temp cache (three versions + two traps),
    /// the bundled sidecar. Returns the fixture and every expected path.
    struct FullFixture {
        fixture: Fixture,
        env: PathBuf,
        path_tier: PathBuf,
        local_flat: PathBuf,
        local_pnpm: PathBuf,
        local_bin: PathBuf,
        ancestor_flat: PathBuf,
        cache_new: PathBuf,
        cache_old: PathBuf,
        bundled: PathBuf,
        project_root: PathBuf,
        cache_root: PathBuf,
        exe: PathBuf,
    }

    fn full_fixture(tag: &str) -> FullFixture {
        let fixture = Fixture::new(tag);
        let host = host_platform().unwrap();
        let pkg_rel = host_package_rel();

        let env = fixture.file(Path::new("override").join(host.executable).as_path());
        let path_tier = fixture.file(Path::new("sharedbin").join(host.executable).as_path());

        let project_root = fixture.path("proj/sub");
        std::fs::create_dir_all(&project_root).unwrap();
        let nm = Path::new("proj/node_modules");
        let local_flat = fixture.file(&nm.join(&pkg_rel));
        let local_bin = fixture.file(&nm.join(".bin").join(host.bin_shim));
        let local_pnpm = fixture.file(
            &nm.join(".pnpm")
                .join(format!(
                    "@typescript+typescript-{}-{}@7.0.2",
                    host.npm_os, host.npm_arch
                ))
                .join("node_modules")
                .join(&pkg_rel),
        );
        let ancestor_flat = fixture.file(&Path::new("node_modules").join(&pkg_rel));

        let cache_root = fixture.path("cache");
        let cache_dir = |version: &str| {
            Path::new("cache")
                .join(CACHE_DIR_NAME)
                .join(user_cache_id())
                .join(host.target_triple)
                .join(crate::toolchain::policy::SUPPORTED_POLICY_ID)
                .join(version)
        };
        let cache_new = fixture.file(
            &cache_dir("7.0.9")
                .join("package")
                .join("lib")
                .join(host.executable),
        );
        fixture.file(&cache_dir("7.0.9").join(READY_MARKER));
        let cache_old = fixture.file(
            &cache_dir("7.0.3")
                .join("package")
                .join("lib")
                .join(host.executable),
        );
        fixture.file(&cache_dir("7.0.3").join(READY_MARKER));
        // Traps: an out-of-range version and a prerelease must be skipped.
        fixture.file(
            &cache_dir("7.1.0")
                .join("package")
                .join("lib")
                .join(host.executable),
        );
        fixture.file(&cache_dir("7.1.0").join(READY_MARKER));
        fixture.file(
            &cache_dir("7.0.2-rc.1")
                .join("package")
                .join("lib")
                .join(host.executable),
        );
        fixture.file(&cache_dir("7.0.2-rc.1").join(READY_MARKER));
        // Trap: a supported version WITHOUT the ready marker is not complete.
        fixture.file(
            &cache_dir("7.0.4")
                .join("package")
                .join("lib")
                .join(host.executable),
        );

        let exe = fixture.file(Path::new("exedir").join("verter-tsc").as_path());
        let bundled = fixture.file(&Path::new("exedir").join(host.bundled_executable_rel_path()));

        FullFixture {
            fixture,
            env,
            path_tier,
            local_flat,
            local_pnpm,
            local_bin,
            ancestor_flat,
            cache_new,
            cache_old,
            bundled,
            project_root,
            cache_root,
            exe,
        }
    }

    impl FullFixture {
        fn request(&self) -> ResolutionRequest {
            request(
                Some(self.project_root.clone()),
                Some(self.env.clone()),
                vec![self.fixture.path("sharedbin")],
                Some(self.cache_root.clone()),
                Some(self.exe.clone()),
            )
        }
    }

    // ── DISCRIMINATING: the enumeration order IS the tier order — shared
    //    (env, then PATH), then project-local (nearest first), then cache
    //    (newest first), then bundled. A regression in tier precedence
    //    reorders or drops an entry here. ─────────────────────────────────────
    #[test]
    fn enumeration_walks_tiers_in_order() {
        let f = full_fixture("order");
        let enumeration = enumerate_candidates(&f.request());
        // Real-FS ancestors of the temp dir could theoretically contribute
        // extra local candidates AFTER the fixture's own; restrict the exact
        // assertion to the fixture subtree.
        let paths: Vec<&Path> = enumeration
            .candidates
            .iter()
            .filter(|c| c.path.starts_with(&f.fixture.root))
            .map(|c| c.path.as_path())
            .collect();
        assert_eq!(
            paths,
            vec![
                f.env.as_path(),
                f.path_tier.as_path(),
                f.local_flat.as_path(),
                f.local_pnpm.as_path(),
                f.local_bin.as_path(),
                f.ancestor_flat.as_path(),
                f.cache_new.as_path(),
                f.cache_old.as_path(),
                f.bundled.as_path(),
            ],
            "tier order: env → PATH → local (flat, pnpm, .bin) → ancestor → cache (new→old) → bundled"
        );
    }

    // ── DISCRIMINATING: foreign-platform packages are NEVER candidates (the
    //    historical bug: the old resolver appended every other platform's
    //    suffix after the host's). ────────────────────────────────────────────
    #[test]
    fn foreign_platform_packages_are_never_enumerated() {
        let fixture = Fixture::new("foreign");
        let host = host_platform().unwrap();
        // Install ONLY a foreign platform package.
        let foreign = crate::toolchain::platform::PLATFORM_MANIFEST
            .iter()
            .find(|p| p.npm_os != host.npm_os || p.npm_arch != host.npm_arch)
            .unwrap();
        let nm = Path::new("proj/node_modules/@typescript");
        fixture.file(
            &nm.join(format!(
                "typescript-{}-{}",
                foreign.npm_os, foreign.npm_arch
            ))
            .join("lib")
            .join(foreign.executable),
        );
        let enumeration = enumerate_candidates(&request(
            Some(fixture.path("proj")),
            None,
            vec![],
            None,
            None,
        ));
        assert!(
            enumeration.candidates.is_empty(),
            "a foreign-platform binary must never be a candidate: {:?}",
            enumeration.candidates
        );
    }

    // ── DISCRIMINATING: the `.bin` shim and the pnpm store are found when
    //    they are the ONLY local installs. ────────────────────────────────────
    #[test]
    fn bin_shim_is_found_when_the_package_is_absent() {
        let fixture = Fixture::new("shim");
        let host = host_platform().unwrap();
        let shim = fixture.file(&Path::new("proj/node_modules/.bin").join(host.bin_shim));
        let enumeration = enumerate_candidates(&request(
            Some(fixture.path("proj")),
            None,
            vec![],
            None,
            None,
        ));
        assert_eq!(enumeration.candidates.len(), 1);
        assert_eq!(enumeration.candidates[0].path, shim);
        assert_eq!(
            enumeration.candidates[0].provenance,
            Provenance::ProjectLocal
        );
    }

    // ── DISCRIMINATING: a stale env override is skipped WITH A NOTE (never
    //    silently trusted, never a hard failure). ─────────────────────────────
    #[test]
    fn stale_env_override_is_skipped_with_a_note() {
        let fixture = Fixture::new("stale");
        let stale = fixture.path("no/such/tsc");
        let enumeration =
            enumerate_candidates(&request(None, Some(stale.clone()), vec![], None, None));
        assert!(enumeration.candidates.is_empty());
        assert!(
            enumeration
                .notes
                .iter()
                .any(|n| n.contains(ENV_OVERRIDE_VAR) && n.contains("not a usable file")),
            "the note must name the var and why it was skipped: {:?}",
            enumeration.notes
        );
    }

    // ── DISCRIMINATING: the same file reached through two tiers is ONE
    //    candidate (canonicalized dedup). ─────────────────────────────────────
    #[test]
    fn duplicate_candidates_are_enumerated_once() {
        let f = full_fixture("dedup");
        let mut req = f.request();
        // Point the env override at the same file PATH traversal finds.
        req.env_override = Some(f.path_tier.clone());
        let enumeration = enumerate_candidates(&req);
        let path_tier_hits = enumeration
            .candidates
            .iter()
            .filter(|c| c.path == f.path_tier)
            .count();
        assert_eq!(
            path_tier_hits, 1,
            "identical files must dedup to one candidate"
        );
        // And the surviving candidate is the env-tier one (higher precedence).
        assert_eq!(
            enumeration.candidates[0].provenance,
            Provenance::EnvOverride
        );
    }

    // ── DISCRIMINATING (H10): the PATH tier enumerates EXACTLY the names the
    //    platform manifest publishes — per-OS binary names have ONE source
    //    (the manifest), never a hardcoded branch in discovery. ────────────────
    #[test]
    fn path_tier_enumerates_exactly_the_manifest_names() {
        let fixture = Fixture::new("pathnames");
        let host = host_platform().unwrap();
        let dir = fixture.path("sharedbin");
        std::fs::create_dir_all(&dir).unwrap();
        for name in host.path_executable_names() {
            fixture.file(&Path::new("sharedbin").join(name));
        }
        let enumeration = enumerate_candidates(&request(None, None, vec![dir.clone()], None, None));
        let mut found: Vec<String> = enumeration
            .candidates
            .iter()
            .map(|c| c.path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        found.sort();
        let mut expected: Vec<String> = host
            .path_executable_names()
            .iter()
            .map(|s| s.to_string())
            .collect();
        expected.sort();
        expected.dedup();
        assert_eq!(
            found, expected,
            "the PATH tier must enumerate exactly the manifest's names"
        );
        assert!(
            enumeration
                .candidates
                .iter()
                .all(|c| c.provenance == Provenance::SharedPath),
            "PATH hits are SharedPath: {:?}",
            enumeration.candidates
        );
    }

    // ── DISCRIMINATING (H10): the legacy `.bin` shim is found via the
    //    manifest's legacy name (no duplicated OS branch in discovery). ────────
    #[test]
    fn legacy_bin_shim_is_found_via_the_manifest_name() {
        let fixture = Fixture::new("legacyshim");
        let host = host_platform().unwrap();
        let shim = fixture.file(&Path::new("proj/node_modules/.bin").join(host.legacy_bin_shim()));
        let enumeration = enumerate_candidates(&request(
            Some(fixture.path("proj")),
            None,
            vec![],
            None,
            None,
        ));
        assert!(
            enumeration
                .candidates
                .iter()
                .any(|c| c.path == shim && c.provenance == Provenance::ProjectLocal),
            "the legacy shim at the manifest's name must be enumerated: {:?}",
            enumeration.candidates
        );
    }

    // ── DISCRIMINATING (B7): the full fixture's `.bin` candidate IS the
    //    platform `bin_shim` name — production enumerates `bin_shim`, so a
    //    fixture planting any other name (e.g. the package executable)
    //    silently mismatches the accept list on Windows. Tie them here. ────────
    #[test]
    fn full_fixture_bin_candidate_matches_the_platform_bin_shim() {
        let f = full_fixture("binshim");
        let host = host_platform().unwrap();
        assert_eq!(
            f.local_bin.file_name().unwrap().to_string_lossy().as_ref(),
            host.bin_shim,
            "the fixture's `.bin` file name must equal the platform bin_shim"
        );
        let enumeration = enumerate_candidates(&f.request());
        assert!(
            enumeration
                .candidates
                .iter()
                .any(|c| c.path == f.local_bin && c.provenance == Provenance::ProjectLocal),
            "the fixture `.bin` candidate must be enumerated: {:?}",
            enumeration.candidates
        );
    }

    // ── DISCRIMINATING: a symlinked cache root component rejects the whole
    //    cache tier (a trust boundary), with a note; the walk falls through. ──
    #[cfg(unix)]
    #[test]
    fn symlinked_cache_tree_skips_the_tier() {
        let f = full_fixture("symlink");
        // Replace <cache>/verter-tsgo-v1 with a symlink to the real tree.
        let link = f.cache_root.join(CACHE_DIR_NAME);
        let target = f.fixture.path("real-cache-tree");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::remove_dir_all(&link).unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let enumeration = enumerate_candidates(&f.request());
        assert!(
            !enumeration
                .candidates
                .iter()
                .any(|c| c.provenance == Provenance::TempCache),
            "a symlinked cache tree must not contribute candidates"
        );
        assert!(
            enumeration.notes.iter().any(|n| n.contains("symlink")),
            "the skipped tier must be explained: {:?}",
            enumeration.notes
        );
    }

    // ── DISCRIMINATING (B4a): a bundled sidecar that EXISTS but is
    //    structurally invalid (a symlink/reparse component) is a
    //    PRODUCT-INTEGRITY failure — never a soft skip that falls through to
    //    "no provider". RED: today it degrades to a note + NoUsableCandidate. ──
    #[cfg(unix)]
    #[tokio::test]
    async fn a_symlinked_bundled_sidecar_is_a_product_integrity_error() {
        let f = full_fixture("bundlesymlink");
        // Replace the bundled binary with a symlink to a real file elsewhere
        // (a tampered install: the sidecar location is no longer a real file).
        let target = f.fixture.file(Path::new("smuggled/engine"));
        std::fs::remove_file(&f.bundled).unwrap();
        std::os::unix::fs::symlink(&target, &f.bundled).unwrap();

        let validator = ScriptedValidator::accepting(&[]);
        let err = resolve_with(&f.request(), &validator)
            .await
            .expect_err("nothing validates and the bundled sidecar is invalid");
        match &err {
            ResolveError::ProductIntegrity { path, reason } => {
                assert_eq!(path, &f.bundled, "the invalid sidecar must be named");
                assert!(
                    matches!(reason.as_ref(), RejectionReason::UntrustedLocation { .. }),
                    "the reason must name the structural trust failure: {reason:?}"
                );
            }
            other => panic!(
                "a present-but-invalid bundled sidecar must be ProductIntegrity (loud \
                 reinstall signal), never a soft no-provider miss: {other:?}"
            ),
        }
        let rendered = err.to_string();
        assert!(rendered.contains("product integrity"), "{rendered}");
        assert!(rendered.contains("Reinstall"), "{rendered}");
    }

    // ── DISCRIMINATING (B4b): canonical-path dedup must not DISCARD Bundled
    //    provenance — PATH/VERTER_TSGO_BIN pointing AT the bundled binary still
    //    classifies as Bundled, so an invalid one escalates to ProductIntegrity
    //    instead of degrading to an ordinary no-provider result. RED: dedup
    //    keeps the FIRST (tier-1) provenance today. ─────────────────────────────
    #[tokio::test]
    async fn dedup_with_an_earlier_tier_keeps_bundled_provenance() {
        let f = full_fixture("bundleddup");
        let mut req = f.request();
        // The env override points AT the bundled sidecar (same file).
        req.env_override = Some(f.bundled.clone());
        let enumeration = enumerate_candidates(&req);
        let hits: Vec<&Candidate> = enumeration
            .candidates
            .iter()
            .filter(|c| c.path == f.bundled)
            .collect();
        assert_eq!(hits.len(), 1, "the same file dedups to one candidate");
        assert_eq!(
            hits[0].provenance,
            Provenance::Bundled,
            "a canonical path reachable as the bundled sidecar must retain Bundled \
             provenance (integrity escalation depends on it)"
        );

        // And its validation failure escalates to ProductIntegrity, not a
        // plain no-provider result.
        let err = resolve_with(&req, &ScriptedValidator::accepting(&[]))
            .await
            .expect_err("the bundled duplicate fails validation");
        assert!(
            matches!(err, ResolveError::ProductIntegrity { .. }),
            "an invalid bundled binary must escalate to ProductIntegrity even when \
             an earlier tier named the same file: {err:?}"
        );
    }

    // ── DISCRIMINATING (B5): a cache entry whose `package`/`lib`/binary
    //    component is a symlink ESCAPING the trusted root is REJECTED — the
    //    trust check covers the FULL resolved path to the binary, not merely
    //    the version directory. RED: today only the version dir is checked and
    //    the symlinked `package` is followed into an accepted candidate. ──────
    #[cfg(unix)]
    #[test]
    fn a_cache_binary_with_a_symlinked_component_escaping_the_root_is_rejected() {
        let f = full_fixture("cacheescape");
        // <cache>/…/<policy>/7.0.9/package → symlink to a tree OUTSIDE the cache.
        let version_dir = f
            .cache_new
            .parent() // lib
            .and_then(Path::parent) // package
            .and_then(Path::parent) // <version>
            .unwrap()
            .to_path_buf();
        let host = host_platform().unwrap();
        let outside = f.fixture.path("outside-tree");
        std::fs::create_dir_all(outside.join("lib")).unwrap();
        std::fs::write(outside.join("lib").join(host.executable), b"escaped").unwrap();
        std::fs::remove_dir_all(version_dir.join("package")).unwrap();
        std::os::unix::fs::symlink(&outside, version_dir.join("package")).unwrap();

        let enumeration = enumerate_candidates(&f.request());
        assert!(
            !enumeration
                .candidates
                .iter()
                .any(|c| c.path.starts_with(&version_dir)),
            "a cache binary reached through a symlinked `package` must be rejected: {:?}",
            enumeration.candidates
        );
        // The clean 7.0.3 entry still resolves (per-entry rejection, not a
        // whole-tier skip).
        assert!(
            enumeration.candidates.iter().any(|c| c.path == f.cache_old),
            "the untouched in-tree cache entry must survive: {:?}",
            enumeration.candidates
        );
    }

    // ── DISCRIMINATING (B5): the Unix trusted root must be OWNER-VALIDATED —
    //    a root owned by someone else is rejected (only group/world write bits
    //    were checked before); a clean, current-user-owned root is accepted. ──
    #[cfg(unix)]
    #[test]
    fn a_cache_root_owned_by_another_user_is_rejected() {
        let f = full_fixture("cacheowner");
        let v1_root = f.cache_root.join(CACHE_DIR_NAME);
        let euid = unsafe { libc::geteuid() };
        assert_eq!(
            unix_cache_root_trust_issues(&v1_root, euid),
            None,
            "a clean, current-user-owned cache root must be trusted"
        );
        let issue = unix_cache_root_trust_issues(&v1_root, euid + 1)
            .expect("a root not owned by the current user must be rejected");
        assert!(issue.contains("owned"), "{issue}");
    }

    // ── DISCRIMINATING (B5): a group/world-WRITABLE cache root skips the whole
    //    tier with an explanatory note (another user could swap engines). ─────
    #[cfg(unix)]
    #[test]
    fn a_world_writable_cache_root_skips_the_tier_with_a_note() {
        use std::os::unix::fs::PermissionsExt;
        let f = full_fixture("cachemode");
        let v1_root = f.cache_root.join(CACHE_DIR_NAME);
        std::fs::set_permissions(&v1_root, std::fs::Permissions::from_mode(0o777)).unwrap();
        let enumeration = enumerate_candidates(&f.request());
        assert!(
            !enumeration
                .candidates
                .iter()
                .any(|c| c.provenance == Provenance::TempCache),
            "a writable cache root must not contribute candidates"
        );
        assert!(
            enumeration.notes.iter().any(|n| n.contains("writable")),
            "the skipped tier must be explained: {:?}",
            enumeration.notes
        );
    }

    // ── scripted-validation resolver tests ───────────────────────────────────

    /// A [`CandidateValidator`] that accepts candidates whose path contains
    /// one of `accept`, recording call order. No processes are spawned.
    struct ScriptedValidator {
        accept: Vec<String>,
        calls: Mutex<Vec<PathBuf>>,
    }

    impl ScriptedValidator {
        fn accepting(accept: &[&str]) -> Self {
            Self {
                accept: accept.iter().map(|s| s.to_string()).collect(),
                calls: Mutex::new(Vec::new()),
            }
        }
        fn calls(&self) -> Vec<PathBuf> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl crate::toolchain::validation::CandidateValidator for ScriptedValidator {
        fn validate<'a>(
            &'a self,
            path: &'a Path,
            _requirement: Capability,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<Output = Result<ValidatedCandidate, RejectionReason>>
                    + Send
                    + 'a,
            >,
        > {
            self.calls.lock().unwrap().push(path.to_path_buf());
            let accepted = self
                .accept
                .iter()
                .any(|s| path.to_string_lossy().contains(s.as_str()));
            Box::pin(async move {
                if accepted {
                    Ok(ValidatedCandidate {
                        path: path.to_path_buf(),
                        version: TsgoVersion::new(7, 0, 2),
                        version_string: "7.0.2".to_string(),
                    })
                } else {
                    Err(RejectionReason::VersionProbeFailed {
                        detail: "scripted rejection".to_string(),
                    })
                }
            })
        }
    }

    // ── DISCRIMINATING: first WORKING wins — a later tier only gets its
    //    chance after every earlier candidate failed validation. ──────────────
    type WinnerCase = (&'static str, fn(&FullFixture) -> &PathBuf, Provenance);

    #[tokio::test]
    async fn first_working_candidate_wins_per_tier() {
        // (accept substring, expected winner field selector)
        let cases: Vec<WinnerCase> = vec![
            ("override", |f| &f.env, Provenance::EnvOverride),
            ("sharedbin", |f| &f.path_tier, Provenance::SharedPath),
            ("node_modules", |f| &f.local_flat, Provenance::ProjectLocal),
            ("7.0.9", |f| &f.cache_new, Provenance::TempCache),
            ("exedir", |f| &f.bundled, Provenance::Bundled),
        ];
        for (accept, pick, provenance) in cases {
            let f = full_fixture("winner");
            let validator = ScriptedValidator::accepting(&[accept]);
            let resolution = resolve_with(&f.request(), &validator)
                .await
                .unwrap_or_else(|e| panic!("accept={accept}: {e}"));
            assert_eq!(resolution.path, *pick(&f), "accept={accept}");
            assert_eq!(resolution.provenance, provenance, "accept={accept}");
            // Every candidate BEFORE the winner was tried and rejected;
            // nothing after the winner was touched.
            let calls = validator.calls();
            let winner_index = calls
                .iter()
                .position(|p| *p == resolution.path)
                .expect("the winner was validated");
            assert_eq!(
                winner_index,
                calls.len() - 1,
                "accept={accept}: validation stopped at the first working candidate"
            );
            assert_eq!(resolution.rejections.len(), winner_index, "accept={accept}");
        }
    }

    // ── DISCRIMINATING: a failing-validation candidate is SKIPPED to the
    //    next tier, and its rejection is recorded with provenance. ───────────
    #[tokio::test]
    async fn a_failing_candidate_is_skipped_with_an_actionable_rejection() {
        let f = full_fixture("skip");
        // Only the project-local flat package validates.
        let validator = ScriptedValidator::accepting(&["@typescript"]);
        let resolution = resolve_with(&f.request(), &validator)
            .await
            .expect("local must win");
        assert_eq!(resolution.path, f.local_flat);
        // env + PATH were tried first, in order, and recorded.
        let calls = validator.calls();
        assert_eq!(&calls[0], &f.env);
        assert_eq!(&calls[1], &f.path_tier);
        assert_eq!(resolution.rejections.len(), 2);
        assert_eq!(
            resolution.rejections[0].candidate.provenance,
            Provenance::EnvOverride
        );
        assert_eq!(
            resolution.rejections[1].candidate.provenance,
            Provenance::SharedPath
        );
        let rendered = resolution.rejections[0].to_string();
        assert!(rendered.contains("VERTER_TSGO_BIN"), "{rendered}");
        assert!(rendered.contains("scripted rejection"), "{rendered}");
    }

    // ── DISCRIMINATING: cache entries are tried NEWEST-FIRST; a corrupt
    //    newest entry falls through to the older supported one. ──────────────
    #[tokio::test]
    async fn cache_entries_are_tried_newest_first() {
        let f = full_fixture("cacheorder");
        let validator = ScriptedValidator::accepting(&["7.0.3"]);
        let resolution = resolve_with(&f.request(), &validator)
            .await
            .expect("the older cache entry must win when the newer fails");
        assert_eq!(resolution.path, f.cache_old);
        let calls = validator.calls();
        let new_pos = calls.iter().position(|p| *p == f.cache_new).unwrap();
        let old_pos = calls.iter().position(|p| *p == f.cache_old).unwrap();
        assert!(new_pos < old_pos, "7.0.9 must be tried before 7.0.3");
        // The out-of-range and prerelease cache entries were NEVER candidates.
        assert!(!calls.iter().any(|p| p.to_string_lossy().contains("7.1.0")));
        assert!(!calls.iter().any(|p| p.to_string_lossy().contains("rc.1")));
        assert!(!calls.iter().any(|p| p.to_string_lossy().contains("7.0.4")));
    }

    // ── DISCRIMINATING: an existing-but-invalid bundled sidecar is a
    //    PRODUCT-INTEGRITY failure, never a plain "no provider". ─────────────
    #[tokio::test]
    async fn invalid_bundled_sidecar_is_a_product_integrity_error() {
        let f = full_fixture("integrity");
        let validator = ScriptedValidator::accepting(&[]); // everything fails
        let err = resolve_with(&f.request(), &validator)
            .await
            .expect_err("all candidates fail");
        match &err {
            ResolveError::ProductIntegrity { path, reason } => {
                assert_eq!(path, &f.bundled);
                assert!(matches!(
                    reason.as_ref(),
                    RejectionReason::VersionProbeFailed { .. }
                ));
            }
            other => panic!("expected ProductIntegrity, got {other:?}"),
        }
        let rendered = err.to_string();
        assert!(rendered.contains("product integrity"), "{rendered}");
        assert!(rendered.contains("Reinstall"), "{rendered}");
    }

    // ── DISCRIMINATING: nothing working + NO bundled binary present is a
    //    NoUsableCandidate whose report lists every tier tried and how to
    //    remediate — actionable, not a bare "not found". ──────────────────────
    #[tokio::test]
    async fn no_usable_candidate_report_is_actionable() {
        let f = full_fixture("none");
        let mut req = f.request();
        req.host_exe = None; // no bundled sidecar at all
        let validator = ScriptedValidator::accepting(&[]);
        let err = resolve_with(&req, &validator)
            .await
            .expect_err("nothing validates");
        match &err {
            ResolveError::NoUsableCandidate { rejections, .. } => {
                // env, PATH, 4 local, 2 cache = 8 fixture rejections (real-FS
                // ancestors of the temp dir could add more; restrict to the
                // fixture subtree).
                let fixture_rejections = rejections
                    .iter()
                    .filter(|r| r.candidate.path.starts_with(&f.fixture.root))
                    .count();
                assert_eq!(fixture_rejections, 8, "{rejections:?}");
            }
            other => panic!("expected NoUsableCandidate, got {other:?}"),
        }
        let rendered = err.to_string();
        assert!(rendered.contains("no usable tsgo engine"), "{rendered}");
        assert!(rendered.contains(">=7.0.2, <7.1.0"), "{rendered}");
        assert!(rendered.contains("VERTER_TSGO_BIN"), "{rendered}");
        assert!(rendered.contains("typescript@"), "{rendered}");
        assert!(rendered.contains("scripted rejection"), "{rendered}");
    }

    // ── the production entry point uses the env-derived policy ──────────────
    #[test]
    fn for_environment_reads_the_process_environment() {
        let req = ResolutionRequest::for_environment(Capability::Api, None);
        // PATH entries come from the real environment; the cache root defaults
        // to the system temp dir; the host exe is this test binary.
        assert_eq!(
            req.cache_root.as_deref(),
            Some(std::env::temp_dir().as_path())
        );
        assert!(req.host_exe.is_some());
        assert_eq!(req.requirement, Capability::Api);
        // The env override mirrors VERTER_TSGO_BIN (unset here → None).
        if std::env::var_os(ENV_OVERRIDE_VAR).is_none() {
            assert!(req.env_override.is_none());
        }
    }

    // ── policy filter inside cache enumeration honors the PRODUCTION window
    //    even though VersionPolicy exists — nightlies are never cached. ───────
    #[test]
    fn cache_enumeration_uses_the_production_policy() {
        let _ = VersionPolicy::production(); // documented contract anchor
        let f = full_fixture("cachepolicy");
        let enumeration = enumerate_candidates(&f.request());
        let cache_versions: Vec<String> = enumeration
            .candidates
            .iter()
            .filter(|c| c.provenance == Provenance::TempCache)
            .map(|c| {
                // <version>/package/lib/<exe> — three parents up to the
                // version directory.
                c.path
                    .parent() // lib
                    .and_then(Path::parent) // package
                    .and_then(Path::parent) // <version>
                    .unwrap()
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        assert_eq!(cache_versions, vec!["7.0.9", "7.0.3"]);
    }
}
