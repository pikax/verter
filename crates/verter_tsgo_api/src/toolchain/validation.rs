//! Candidate validation: every tsgo candidate is proven WORKING before the
//! resolver selects it.
//!
//! Validation is capability-aware (ratified design §2):
//!
//! 1. the candidate is a regular file;
//! 2. a bounded `--version` probe succeeds, parses as strict SemVer, and
//!    satisfies the support policy ([`crate::toolchain::policy`]);
//! 3. a capability smoke proves the required surface on a THROWAWAY spawned
//!    process:
//!    - [`Capability::Lsp`] — spawn `tsc --lsp --stdio`, complete `initialize`,
//!      and require the in-band `serverInfo.version` to AGREE with the probe
//!      (a version string can lie; two independent reports must match);
//!    - [`Capability::Api`] — additionally attach an `--api` session and open
//!      a minimal configured project, requiring a bare-integer snapshot handle
//!      (the version-lie-immune rail in
//!      [`crate::gate::require_integer_snapshot_handle`]).
//!
//! The resolver walks candidates through the [`CandidateValidator`] seam so
//! ordering tests can script acceptance without spawning processes.

use std::fmt;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use super::policy::{PolicyRejection, TsgoVersion, VersionPolicy};
use crate::attach::{spawn_own_lsp_connection, TsgoAttach};
use crate::client::probe_engine_version_bounded;

/// The default bound on a `--version` probe (mirrors the owned-provider
/// probe timeout).
const DEFAULT_PROBE_BOUND: Duration = Duration::from_secs(5);
/// The default bound on a capability smoke (spawn + handshake + optional
/// `--api` snapshot).
const DEFAULT_SMOKE_BOUND: Duration = Duration::from_secs(15);

/// The engine surface a resolved candidate must prove before selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    /// The `tsc --lsp --stdio` surface: spawn, complete `initialize`, and
    /// require the in-band `serverInfo.version` to agree with the `--version`
    /// probe.
    Lsp,
    /// The `--api` checker surface: the LSP handshake PLUS attach an `--api`
    /// session and open a minimal configured project, requiring a bare-integer
    /// snapshot handle.
    Api,
}

impl fmt::Display for Capability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Lsp => write!(f, "--lsp"),
            Self::Api => write!(f, "--api"),
        }
    }
}

/// A candidate that passed validation: its path and its probed, policy-checked
/// version.
#[derive(Debug, Clone)]
pub struct ValidatedCandidate {
    /// The validated engine binary path.
    pub path: PathBuf,
    /// The parsed, policy-accepted version.
    pub version: TsgoVersion,
    /// The raw version string the `--version` probe reported.
    pub version_string: String,
}

/// Why a candidate failed validation. Every variant renders an actionable
/// message (what was tried, what failed, what to do).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RejectionReason {
    /// The candidate path is not a regular file.
    NotARegularFile,
    /// The bounded `--version` probe failed (non-zero exit, timeout, or
    /// unparseable output).
    VersionProbeFailed {
        /// The probe failure detail.
        detail: String,
    },
    /// The probed version does not satisfy the support policy.
    PolicyRejected {
        /// The version string the probe reported.
        version: String,
        /// The policy rejection.
        rejection: PolicyRejection,
    },
    /// The `--lsp --stdio` `initialize` handshake failed.
    LspHandshakeFailed {
        /// The handshake failure detail.
        detail: String,
    },
    /// The in-band `serverInfo.version` disagrees with the `--version` probe.
    ServerInfoVersionMismatch {
        /// What `--version` reported.
        probe: String,
        /// What the initialize result's `serverInfo.version` reported.
        server_info: String,
    },
    /// The `--api` capability check failed (attach, session mint, or the
    /// minimal-project snapshot).
    ApiSmokeFailed {
        /// The smoke failure detail.
        detail: String,
    },
}

impl fmt::Display for RejectionReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotARegularFile => {
                write!(f, "not a regular file (a tsgo engine binary is expected)")
            }
            Self::VersionProbeFailed { detail } => {
                write!(f, "the `--version` probe failed: {detail}")
            }
            Self::PolicyRejected { rejection, .. } => write!(f, "{rejection}"),
            Self::LspHandshakeFailed { detail } => write!(
                f,
                "the `--lsp --stdio` initialize handshake failed: {detail}"
            ),
            Self::ServerInfoVersionMismatch { probe, server_info } => write!(
                f,
                "the `--lsp` serverInfo.version `{server_info}` does not match the \
                 `--version` probe `{probe}` — an engine whose two reports disagree \
                 cannot be trusted"
            ),
            Self::ApiSmokeFailed { detail } => {
                write!(f, "the `--api` capability check failed: {detail}")
            }
        }
    }
}

impl std::error::Error for RejectionReason {}

/// The validation seam the resolver walks candidates through. Boxed-future
/// based so scripted (process-free) validators can drive ordering tests.
pub trait CandidateValidator: Send + Sync {
    /// Validate one candidate for `requirement`. Implementations must be
    /// deterministic and bounded.
    fn validate<'a>(
        &'a self,
        path: &'a Path,
        requirement: Capability,
    ) -> Pin<Box<dyn Future<Output = Result<ValidatedCandidate, RejectionReason>> + Send + 'a>>;
}

/// The production validator: real bounded process probes + capability smokes.
pub struct ProcessValidator {
    policy: VersionPolicy,
    probe_bound: Duration,
    smoke_bound: Duration,
}

impl ProcessValidator {
    /// A validator using the env-derived policy ([`VersionPolicy::from_env`])
    /// and the default bounds.
    pub fn from_env() -> Self {
        Self::with_policy(VersionPolicy::from_env())
    }

    /// A validator with an explicit policy and the default bounds.
    pub fn with_policy(policy: VersionPolicy) -> Self {
        Self {
            policy,
            probe_bound: DEFAULT_PROBE_BOUND,
            smoke_bound: DEFAULT_SMOKE_BOUND,
        }
    }

    /// Override the probe/smoke bounds (tests use short bounds).
    pub fn with_bounds(mut self, probe_bound: Duration, smoke_bound: Duration) -> Self {
        self.probe_bound = probe_bound;
        self.smoke_bound = smoke_bound;
        self
    }

    /// The policy this validator enforces.
    pub fn policy(&self) -> VersionPolicy {
        self.policy
    }

    async fn validate_impl(
        &self,
        path: &Path,
        requirement: Capability,
    ) -> Result<ValidatedCandidate, RejectionReason> {
        // 1. A regular file.
        if !path.is_file() {
            return Err(RejectionReason::NotARegularFile);
        }
        // 2. Bounded `--version` probe + strict SemVer + support policy.
        let probe = probe_engine_version_bounded(path, self.probe_bound)
            .await
            .map_err(|e| RejectionReason::VersionProbeFailed {
                detail: e.to_string(),
            })?;
        let version =
            self.policy
                .check_str(&probe)
                .map_err(|rejection| RejectionReason::PolicyRejected {
                    version: probe.clone(),
                    rejection,
                })?;
        // 3. The capability smoke on a throwaway process.
        self.smoke(path, &probe, requirement).await?;
        Ok(ValidatedCandidate {
            path: path.to_path_buf(),
            version,
            version_string: probe,
        })
    }

    /// The capability smoke: prove the required surface on a throwaway spawned
    /// engine, bounded by `smoke_bound`.
    async fn smoke(
        &self,
        path: &Path,
        probe: &str,
        requirement: Capability,
    ) -> Result<(), RejectionReason> {
        match requirement {
            Capability::Lsp => {
                let staged = stage_smoke_project()
                    .map_err(|detail| RejectionReason::LspHandshakeFailed { detail })?;
                let work = self.lsp_smoke(path, probe, &staged);
                tokio::time::timeout(self.smoke_bound, work)
                    .await
                    .map_err(|_| RejectionReason::LspHandshakeFailed {
                        detail: format!("exceeded {} ms", self.smoke_bound.as_millis()),
                    })?
            }
            Capability::Api => {
                let staged = stage_smoke_project()
                    .map_err(|detail| RejectionReason::ApiSmokeFailed { detail })?;
                let work = self.api_smoke(path, probe, &staged);
                tokio::time::timeout(self.smoke_bound, work)
                    .await
                    .map_err(|_| RejectionReason::ApiSmokeFailed {
                        detail: format!("exceeded {} ms", self.smoke_bound.as_millis()),
                    })?
            }
        }
    }

    /// The `--lsp` smoke: spawn, handshake, require the in-band
    /// `serverInfo.version` to agree with the probe, then tear down.
    async fn lsp_smoke(
        &self,
        path: &Path,
        probe: &str,
        staged: &SmokeProject,
    ) -> Result<(), RejectionReason> {
        let conn = spawn_own_lsp_connection(path, &staged.dir)
            .await
            .map_err(|e| RejectionReason::LspHandshakeFailed {
                detail: e.to_string(),
            })?;
        let result = match conn.lsp_handshake(&staged.uri(), &self.policy).await {
            Ok(clearance) => {
                if versions_agree(probe, &clearance.observed_version) {
                    Ok(())
                } else {
                    Err(RejectionReason::ServerInfoVersionMismatch {
                        probe: probe.to_string(),
                        server_info: clearance.observed_version,
                    })
                }
            }
            Err(e) => Err(RejectionReason::LspHandshakeFailed {
                detail: e.to_string(),
            }),
        };
        conn.terminate().await;
        result
    }

    /// The `--api` smoke: the full owned attach (LSP handshake + `--api`
    /// session mint) plus a minimal configured-project snapshot whose first
    /// handle must be a bare integer (the rail lives inside
    /// `update_snapshot_open_project`).
    async fn api_smoke(
        &self,
        path: &Path,
        probe: &str,
        staged: &SmokeProject,
    ) -> Result<(), RejectionReason> {
        let conn = spawn_own_lsp_connection(path, &staged.dir)
            .await
            .map_err(|e| RejectionReason::ApiSmokeFailed {
                detail: format!("spawn the engine: {e}"),
            })?;
        let attach = TsgoAttach::attach_over_with_policy(conn, &staged.uri(), &self.policy)
            .await
            .map_err(|e| RejectionReason::ApiSmokeFailed {
                detail: e.to_string(),
            })?;
        if !versions_agree(probe, attach.observed_version()) {
            let server_info = attach.observed_version().to_string();
            let _ = attach.teardown().await;
            return Err(RejectionReason::ServerInfoVersionMismatch {
                probe: probe.to_string(),
                server_info,
            });
        }
        let tsconfig = staged.tsconfig_string();
        let result = attach
            .update_snapshot(&tsconfig)
            .await
            .map(|_| ())
            .map_err(|e| RejectionReason::ApiSmokeFailed {
                detail: e.to_string(),
            });
        let _ = attach.teardown().await;
        result
    }
}

impl CandidateValidator for ProcessValidator {
    fn validate<'a>(
        &'a self,
        path: &'a Path,
        requirement: Capability,
    ) -> Pin<Box<dyn Future<Output = Result<ValidatedCandidate, RejectionReason>> + Send + 'a>>
    {
        Box::pin(async move { self.validate_impl(path, requirement).await })
    }
}

/// Whether the probe's version report and the engine's in-band self-report
/// agree (strict SemVer equality; an unparseable report never agrees).
fn versions_agree(probe: &str, server_info: &str) -> bool {
    match (TsgoVersion::parse(probe), TsgoVersion::parse(server_info)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// A minimal configured project staged in a unique temp dir, opened by the
/// `--api` smoke. Removed on drop.
struct SmokeProject {
    dir: PathBuf,
}

impl SmokeProject {
    /// The tsconfig path as a forward-slash wire string.
    fn tsconfig_string(&self) -> String {
        self.dir
            .join("tsconfig.json")
            .to_string_lossy()
            .replace('\\', "/")
    }

    /// A `file://` URI for the staged dir (the LSP `rootUri`).
    fn uri(&self) -> String {
        let normalized = self.dir.to_string_lossy().replace('\\', "/");
        if normalized.starts_with('/') {
            format!("file://{normalized}")
        } else {
            format!("file:///{normalized}")
        }
    }
}

impl Drop for SmokeProject {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Stage the minimal configured project the capability smoke opens.
fn stage_smoke_project() -> Result<SmokeProject, String> {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "verter-tsgo-smoke-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("stage the validation project at {}: {e}", dir.display()))?;
    std::fs::write(
        dir.join("tsconfig.json"),
        "{\n  \"compilerOptions\": {\n    \"strict\": true,\n    \"noEmit\": true,\n    \"skipLibCheck\": true\n  },\n  \"files\": [\"index.ts\"]\n}\n",
    )
    .map_err(|e| format!("write the validation tsconfig: {e}"))?;
    std::fs::write(dir.join("index.ts"), "export {};\n")
        .map_err(|e| format!("write the validation index.ts: {e}"))?;
    Ok(SmokeProject { dir })
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── the probe/self-report agreement check ──────────────────────────────

    #[test]
    fn versions_agree_only_on_exact_semver_equality() {
        assert!(versions_agree("7.0.2", "7.0.2"));
        assert!(versions_agree(
            "7.0.0-dev.20260703.1",
            "7.0.0-dev.20260703.1"
        ));
        assert!(
            !versions_agree("7.0.2", "7.0.9"),
            "a patch disagreement is a mismatch"
        );
        assert!(
            !versions_agree("7.0.2", "7.0.2-rc.1"),
            "a prerelease disagreement is a mismatch"
        );
        assert!(
            !versions_agree("7.0.2", "garbage"),
            "an unparseable self-report can never agree"
        );
        assert!(!versions_agree("garbage", "7.0.2"));
    }

    // ── DISCRIMINATING: rejection diagnostics name the requirement, the
    //    failure, and the remediation — never a bare "invalid". ──────────────
    #[test]
    fn rejection_messages_are_actionable() {
        let missing = RejectionReason::NotARegularFile;
        assert!(missing.to_string().contains("not a regular file"));

        let probe = RejectionReason::VersionProbeFailed {
            detail: "exited with code Some(2)".to_string(),
        };
        assert!(probe.to_string().contains("--version"));
        assert!(probe.to_string().contains("exited with code Some(2)"));

        let policy = RejectionReason::PolicyRejected {
            version: "7.1.0".to_string(),
            rejection: crate::toolchain::policy::PolicyRejection::OutOfSupportedRange {
                version: crate::toolchain::policy::TsgoVersion::new(7, 1, 0),
            },
        };
        let msg = policy.to_string();
        assert!(msg.contains("7.1.0"), "{msg}");
        assert!(msg.contains(">=7.0.2, <7.1.0"), "{msg}");

        let handshake = RejectionReason::LspHandshakeFailed {
            detail: "engine exited before initialize completed".to_string(),
        };
        let msg = handshake.to_string();
        assert!(msg.contains("--lsp --stdio"), "{msg}");
        assert!(
            msg.contains("engine exited before initialize completed"),
            "{msg}"
        );

        let mismatch = RejectionReason::ServerInfoVersionMismatch {
            probe: "7.0.2".to_string(),
            server_info: "7.0.9".to_string(),
        };
        let msg = mismatch.to_string();
        assert!(msg.contains("7.0.2"), "{msg}");
        assert!(msg.contains("7.0.9"), "{msg}");
        assert!(msg.contains("serverInfo"), "{msg}");

        let api = RejectionReason::ApiSmokeFailed {
            detail: "connect the --api pipe".to_string(),
        };
        let msg = api.to_string();
        assert!(msg.contains("--api"), "{msg}");
        assert!(msg.contains("connect the --api pipe"), "{msg}");
    }

    #[test]
    fn capability_labels_name_the_wire_surface() {
        assert_eq!(Capability::Lsp.to_string(), "--lsp");
        assert_eq!(Capability::Api.to_string(), "--api");
    }

    // ── DISCRIMINATING: a nonexistent path and a directory are both rejected
    //    before any process is spawned. ──────────────────────────────────────
    #[tokio::test]
    async fn missing_or_directory_candidate_is_not_a_regular_file() {
        let validator = ProcessValidator::with_policy(VersionPolicy::production());
        let missing = std::path::PathBuf::from("/definitely/not/a/tsgo/binary");
        let err = validator
            .validate(&missing, Capability::Lsp)
            .await
            .expect_err("a missing candidate must be rejected");
        assert!(matches!(err, RejectionReason::NotARegularFile), "{err:?}");

        let dir = std::env::temp_dir();
        let err = validator
            .validate(dir.as_path(), Capability::Api)
            .await
            .expect_err("a directory must be rejected");
        assert!(matches!(err, RejectionReason::NotARegularFile), "{err:?}");
    }

    // ── the validator honors the injected policy (production vs dev
    //    override) — the process-level nightly coverage lives in the
    //    integration suite; here the policy object is what flows through. ────
    #[test]
    fn validator_carries_the_injected_policy() {
        let production = ProcessValidator::with_policy(VersionPolicy::production());
        assert!(!production.policy().allows_dev_nightly());
        let dev = ProcessValidator::with_policy(VersionPolicy::with_dev_nightly_override());
        assert!(dev.policy().allows_dev_nightly());
    }
}
