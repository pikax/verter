//! The local-IPC control endpoint: the shim-side listener + the client-side
//! connect, plus the endpoint-path minting.
//!
//! The control protocol rides same-user local IPC — a Windows named pipe or a
//! Unix-domain socket — selected PLATFORM-AWARE (never a hardcoded per-OS
//! literal). The shim BINDS a [`ControlListener`] and accepts control
//! connections; a client connects with [`connect_control_endpoint`] (which
//! reuses the crate's portable pipe/UDS client connect). Each accepted /
//! connected endpoint is split into boxed [`AsyncRead`] / [`AsyncWrite`] halves
//! the control server/client drives with the crate's JSON-RPC framing.

use tokio::io::{AsyncRead, AsyncWrite};

use crate::error::{TsgoApiError, TsgoApiResult};
use crate::transport::pipe_attach::{connect_attach_pipe, AttachReadHalf, AttachWriteHalf};

use super::advertisement::sanitize_component;

/// A boxed read half of a control connection.
pub type ControlReadHalf = AttachReadHalf;
/// A boxed write half of a control connection.
pub type ControlWriteHalf = AttachWriteHalf;

/// The `sockaddr_un.sun_path` budget the Unix control socket path must fit. It is ~104–108 bytes
/// across Unixes; we stay well under it so the minted path — AND any system-temp-dir fallback —
/// is bindable + connectable everywhere. Both the path-minting and the pre-bind budget gate key
/// off it, so a long fallback base cannot slip an unbindable path past the check.
#[cfg(unix)]
const UDS_SUN_PATH_BUDGET: usize = 100;

/// The basename prefix of the PRIVATE per-session subdir the listener creates for the control
/// socket (`vr-ctl-<hash>`). The path-minting and the listener's `Drop` cleanup both key off it,
/// so the now-empty subdir can be removed on teardown without ever touching `control_dir`.
#[cfg(unix)]
const PRIVATE_SUBDIR_PREFIX: &str = "vr-ctl-";

/// Mint the control endpoint path for `(session_key, pid, disambiguator)`.
///
/// - Windows: a named pipe `\\.\pipe\verter-relay-ctl-<session>-<pid>-<disamb>`
///   (the `\\.\pipe\` namespace, not the filesystem; the name segment is
///   sanitized, so no backslash / NTFS-illegal character reaches it).
/// - Unix: a UDS path under `control_dir`
///   (`verter-relay-ctl-<session>-<pid>-<disamb>.sock`); if that would exceed
///   the `sockaddr_un` path budget it falls back to the system temp dir with a
///   short hashed name, keeping the socket connectable everywhere.
///
/// `control_dir` is unused on Windows (the pipe lives in the pipe namespace);
/// the parameter is accepted uniformly so the caller does not branch.
#[must_use]
pub fn control_endpoint_path(
    control_dir: &std::path::Path,
    session_key: &str,
    pid: u32,
    disambiguator: &str,
) -> String {
    let session = sanitize_component(session_key);
    let disamb = sanitize_component(disambiguator);
    #[cfg(windows)]
    {
        let _ = control_dir;
        format!(r"\\.\pipe\verter-relay-ctl-{session}-{pid}-{disamb}")
    }
    #[cfg(unix)]
    {
        // The `sockaddr_un.sun_path` budget is ~104–108 bytes across Unixes; stay well under
        // it. The socket's IMMEDIATE parent is ALWAYS a PRIVATE per-session subdir
        // (`vr-ctl-<hash>`) created owner-only (`0o700`) at bind — NEVER `control_dir` itself,
        // which may be a legitimately-shared `0o755` dir. Try `control_dir`; if the
        // descriptive path would blow the budget, fall back to the same private-subdir shape
        // under the system temp dir.
        let subdir = format!(
            "{PRIVATE_SUBDIR_PREFIX}{:016x}",
            super::advertisement::stable_hash_str(&format!("{session}-{pid}-{disamb}"))
        );
        let name = format!("verter-relay-ctl-{session}-{pid}-{disamb}.sock");
        let in_dir = control_dir.join(&subdir).join(&name);
        if in_dir.as_os_str().len() <= UDS_SUN_PATH_BUDGET {
            return in_dir.to_string_lossy().into_owned();
        }
        let short = std::env::temp_dir().join(&subdir).join("ctl.sock");
        short.to_string_lossy().into_owned()
    }
    #[cfg(not(any(windows, unix)))]
    {
        let _ = (control_dir, pid);
        format!("verter-relay-ctl-{session}-{disamb}")
    }
}

/// Fail closed if the control socket path exceeds the `sockaddr_un.sun_path` budget. The
/// path-minting prefers `control_dir` and falls back to the system temp dir, but a very long temp
/// dir (or `--control-dir`) can itself blow the budget; reject it here — BEFORE any directory is
/// created — with an actionable error instead of handing `UnixListener::bind` an unbindable /
/// unconnectable path and an opaque OS error.
#[cfg(unix)]
fn check_uds_path_budget(endpoint: &str) -> TsgoApiResult<()> {
    if endpoint.len() > UDS_SUN_PATH_BUDGET {
        return Err(TsgoApiError::Transport(format!(
            "control unix socket path {endpoint:?} is {} bytes, over the {UDS_SUN_PATH_BUDGET}-byte \
             sockaddr_un budget; use a shorter --control-dir (or TMPDIR)",
            endpoint.len()
        )));
    }
    Ok(())
}

/// Whether the control-socket grandparent (`--control-dir`) meets the secure-permissions ceiling,
/// given its owner `uid`, its `st_mode`, and the current `euid`. The grandparent is SAFE iff EITHER:
///
/// - (A) it is owned by the current euid AND has no group/other WRITE bits — a private directory we
///   own, so no other local user can rename/swap our validated `0o700` subdir; OR
/// - (B) it is sticky (`0o1000` set) AND owned by the current euid OR by root (uid `0`). In a sticky
///   directory only an entry's owner — plus the directory's owner and root — may rename/delete it,
///   so `/tmp` (root-owned `0o1777`) and a dir we own with the sticky bit are safe, but a sticky
///   directory owned by ANOTHER non-root user is NOT: that owner could rename/delete our subdir
///   despite the sticky bit.
///
/// Every other case FAILS CLOSED (returns `false`). A pure predicate over `(uid, mode, euid)` — no
/// syscalls — so the ceiling is unit-testable on every platform without needing root or a real
/// other-user-owned directory; it is called only from the `#[cfg(unix)]` bind path.
#[cfg_attr(not(unix), allow(dead_code))]
fn grandparent_ceiling_ok(uid: u32, mode: u32, euid: u32) -> bool {
    let owned_by_euid = uid == euid;
    // (A) a private directory we own, with no group/other WRITE bits (`0o022`).
    let owned_no_foreign_write = owned_by_euid && mode & 0o022 == 0;
    // (B) sticky (`0o1000`) AND owned by a trusted owner — us or root (`uid` 0); a sticky directory
    // owned by another non-root user is rejected (that owner could still rename/delete our subdir).
    let sticky_trusted_owner = mode & 0o1000 != 0 && (owned_by_euid || uid == 0);
    owned_no_foreign_write || sticky_trusted_owner
}

/// Create + validate the PRIVATE per-session parent directory of a Unix control socket BEFORE
/// bind. The socket's IMMEDIATE parent is a subdir WE create owner-only (`0o700`); its
/// grandparent (`--control-dir`) only needs to EXIST + be traversable — it may be a
/// legitimately-shared `0o755` dir, so it is created recursively if missing but NEVER
/// tightened. The private subdir must end up a REAL directory (not a symlink), OWNED by the
/// current euid, at EXACTLY mode `0o700` — so no other local user can pre-create, symlink-swap,
/// own, or traverse into the socket's parent, and an owner-inaccessible pre-create (`0o000` /
/// `0o600`) is rejected too.
///
/// The grandparent is additionally held to the standard secure-permissions ceiling (see
/// `grandparent_ceiling_ok`): it is safe iff EITHER (A) owned by the current euid with no
/// group/other write bits, OR (B) sticky (like `/tmp` at `0o1777`) AND owned by the current euid or
/// by root. A sticky grandparent owned by ANOTHER non-root user is rejected — that owner could
/// rename/swap our validated `0o700` subdir between validate and bind despite the sticky bit.
///
/// Full open-time TOCTOU-proofing is bounded by our ownership of the created subdir and this
/// grandparent ceiling: `tokio::net::UnixListener::bind` takes a PATH (there is no dir-fd bind),
/// so a path-based bind cannot be atomic against an attacker who can already write the grandparent.
/// The euid + exact-`0o700` + real-directory gate on the subdir we own, plus the (A)/(B)
/// ownership/sticky ceiling on the grandparent, is the accepted, standard mitigation.
#[cfg(unix)]
fn prepare_unix_socket_parent(endpoint: &str) -> TsgoApiResult<()> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};

    let socket_parent = std::path::Path::new(endpoint).parent().ok_or_else(|| {
        TsgoApiError::Transport(format!(
            "control unix socket {endpoint:?} has no parent directory"
        ))
    })?;

    // SAFETY: `geteuid` reads the calling process's effective uid; it never fails and has no
    // preconditions.
    let euid = unsafe { libc::geteuid() };

    // The grandparent (`--control-dir`) only needs to EXIST + be traversable — create it
    // recursively with DEFAULT perms if missing (never force `0o700` on a possibly-shared
    // dir). The privacy guarantee lives on the per-session subdir below, not on control_dir.
    if let Some(control_dir) = socket_parent.parent() {
        if !control_dir.as_os_str().is_empty() {
            std::fs::create_dir_all(control_dir).map_err(|e| {
                TsgoApiError::Transport(format!("create control dir {control_dir:?} failed: {e}"))
            })?;
            // Secure-permissions ceiling (OpenSSH-style), encoded by `grandparent_ceiling_ok`: the
            // grandparent is SAFE iff EITHER (A) owned by us with no group/other WRITE bits, OR (B)
            // sticky (`0o1000`) AND owned by us or by root. A sticky grandparent owned by another
            // non-root user is rejected — that owner could rename/swap our `0o700` subdir despite
            // the sticky bit — as is any group/other-writable non-sticky dir (fail closed).
            // `metadata` follows symlinks, so a legitimately-symlinked base (e.g. macOS `/tmp`)
            // resolves to its real perms. `UnixListener::bind` is path-based (there is no dir-fd
            // bind), so this ownership/sticky ceiling — not an open-time atomic bind — is the
            // achievable, standard mitigation for the grandparent-swap TOCTOU.
            let gp_meta = std::fs::metadata(control_dir).map_err(|e| {
                TsgoApiError::Transport(format!("stat control dir {control_dir:?} failed: {e}"))
            })?;
            let gp_mode = gp_meta.permissions().mode();
            let gp_uid = gp_meta.uid();
            if !grandparent_ceiling_ok(gp_uid, gp_mode, euid) {
                return Err(TsgoApiError::Transport(format!(
                    "control dir {control_dir:?} is unsafe (mode {:#o}, owner uid {gp_uid}): it \
                     must be a dir you own with no group/other write bits, or a sticky dir (like \
                     the system temp dir) owned by you or root. A group/other-writable non-sticky \
                     dir — or a sticky dir owned by another non-root user — lets another local user \
                     swap the control socket's private parent.",
                    gp_mode & 0o7777
                )));
            }
        }
    }

    // Create the PRIVATE per-session subdir owner-only (`0o700`), NON-recursively (the
    // grandparent exists), so we are its creator. A pre-existing subdir is VALIDATED below
    // (it may be a hostile pre-create) rather than trusted.
    match std::fs::DirBuilder::new().mode(0o700).create(socket_parent) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => {
            return Err(TsgoApiError::Transport(format!(
                "create private control socket parent {socket_parent:?} failed: {e}"
            )));
        }
    }

    // Validate the FINAL subdir: `symlink_metadata` does NOT follow symlinks, so a symlink (or
    // any non-directory) is rejected; the euid ownership + exact-`0o700` checks reject a parent
    // owned by — or accessible to — another local user.
    let meta = std::fs::symlink_metadata(socket_parent).map_err(|e| {
        TsgoApiError::Transport(format!(
            "stat control socket parent {socket_parent:?} failed: {e}"
        ))
    })?;
    if !meta.file_type().is_dir() {
        return Err(TsgoApiError::Transport(format!(
            "control socket parent {socket_parent:?} is not a real directory (a symlink or non-directory)"
        )));
    }
    if meta.uid() != euid {
        return Err(TsgoApiError::Transport(format!(
            "control socket parent {socket_parent:?} is owned by uid {} not the current euid \
             {euid} (a possible pre-create by another user)",
            meta.uid()
        )));
    }
    // Require EXACTLY `0o700` (owner rwx, no group/other). `mode & 0o077 == 0` alone would wrongly
    // ACCEPT an owner-inaccessible `0o000` / `0o600` pre-create, contradicting the promised private
    // traversable parent — reject anything that is not owner-rwx-only.
    let mode = meta.permissions().mode();
    if mode & 0o777 != 0o700 {
        return Err(TsgoApiError::Transport(format!(
            "control socket parent {socket_parent:?} is not private 0o700 (mode {:#o} is not \
             owner-rwx-only)",
            mode & 0o7777
        )));
    }
    Ok(())
}

/// Remove a stale socket file at `endpoint`, tolerating ONLY `NotFound`: any other unlink
/// error (a permission failure, a security-relevant collision) is a bind failure, never
/// silently swallowed.
#[cfg(unix)]
fn unlink_stale_control_socket(endpoint: &str) -> TsgoApiResult<()> {
    match std::fs::remove_file(endpoint) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(TsgoApiError::Transport(format!(
            "remove stale control socket {endpoint:?} failed: {e}"
        ))),
    }
}

/// Restrict the bound control socket to owner-only (`0o600`); a chmod failure is a bind
/// failure (the socket must never be left group/other-accessible).
#[cfg(unix)]
fn set_control_socket_permissions(endpoint: &str) -> TsgoApiResult<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(endpoint, std::fs::Permissions::from_mode(0o600)).map_err(|e| {
        TsgoApiError::Transport(format!(
            "set control socket {endpoint:?} permissions to 0o600 failed: {e}"
        ))
    })
}

/// The shim-side control endpoint listener: a Windows named-pipe server or a
/// Unix-domain-socket listener, bound to the endpoint the shim advertises.
/// Accepts control connections one instance at a time.
pub struct ControlListener {
    endpoint: String,
    #[cfg(windows)]
    server: Option<tokio::net::windows::named_pipe::NamedPipeServer>,
    #[cfg(unix)]
    listener: tokio::net::UnixListener,
}

impl ControlListener {
    /// Bind the control endpoint. Must be called within a tokio runtime (the
    /// listener registers with the reactor). On Unix a stale socket file at the
    /// path is removed first (best-effort) so a re-bind after an unclean exit
    /// still succeeds.
    pub fn bind(endpoint: &str) -> TsgoApiResult<Self> {
        #[cfg(windows)]
        {
            use tokio::net::windows::named_pipe::ServerOptions;
            let server = ServerOptions::new()
                .first_pipe_instance(true)
                .create(endpoint)
                .map_err(|e| {
                    TsgoApiError::Transport(format!(
                        "bind control named pipe {endpoint:?} failed: {e}"
                    ))
                })?;
            Ok(Self {
                endpoint: endpoint.to_string(),
                server: Some(server),
            })
        }
        #[cfg(unix)]
        {
            // Re-check the FULL path against the sockaddr_un budget (covers a long temp-dir
            // fallback) BEFORE creating any directory, then create + validate the PRIVATE
            // per-session parent dir (exactly 0o700, real dir, euid-owned) under a sticky/owned-safe
            // grandparent, then bind and lock the socket down to 0o600.
            check_uds_path_budget(endpoint)?;
            prepare_unix_socket_parent(endpoint)?;
            unlink_stale_control_socket(endpoint)?;
            let listener = tokio::net::UnixListener::bind(endpoint).map_err(|e| {
                TsgoApiError::Transport(format!(
                    "bind control unix socket {endpoint:?} failed: {e}"
                ))
            })?;
            // A chmod failure after a successful bind must NOT leak the socket file: no
            // `ControlListener` is constructed yet, so its `Drop` cleanup would never run.
            // Remove the just-bound socket before propagating the error (fail closed).
            if let Err(e) = set_control_socket_permissions(endpoint) {
                let _ = std::fs::remove_file(endpoint);
                return Err(e);
            }
            Ok(Self {
                endpoint: endpoint.to_string(),
                listener,
            })
        }
        #[cfg(not(any(windows, unix)))]
        {
            let _ = endpoint;
            Err(TsgoApiError::Transport(
                "the control endpoint is only implemented for Windows and Unix".to_string(),
            ))
        }
    }

    /// The bound endpoint path (the value written into the advertisement).
    #[must_use]
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    /// Accept the next control connection, returning its split read/write halves.
    /// On Windows the next pipe instance is provisioned before returning, so a
    /// subsequent client can connect while the current one is served.
    pub async fn accept(&mut self) -> TsgoApiResult<(ControlReadHalf, ControlWriteHalf)> {
        #[cfg(windows)]
        {
            use tokio::net::windows::named_pipe::ServerOptions;
            let server = self.server.take().ok_or_else(|| {
                TsgoApiError::Transport("control listener has no pending pipe instance".to_string())
            })?;
            server.connect().await.map_err(|e| {
                TsgoApiError::Transport(format!("control named pipe accept failed: {e}"))
            })?;
            // Provision the next instance for a subsequent client.
            let next = ServerOptions::new().create(&self.endpoint).map_err(|e| {
                TsgoApiError::Transport(format!("provision next control pipe instance failed: {e}"))
            })?;
            self.server = Some(next);
            let (read, write) = tokio::io::split(server);
            Ok((boxed_read(read), boxed_write(write)))
        }
        #[cfg(unix)]
        {
            let (stream, _addr) = self.listener.accept().await.map_err(|e| {
                TsgoApiError::Transport(format!("control unix socket accept failed: {e}"))
            })?;
            let (read, write) = tokio::io::split(stream);
            Ok((boxed_read(read), boxed_write(write)))
        }
        #[cfg(not(any(windows, unix)))]
        {
            Err(TsgoApiError::Transport(
                "the control endpoint is only implemented for Windows and Unix".to_string(),
            ))
        }
    }
}

impl Drop for ControlListener {
    fn drop(&mut self) {
        // On Unix the socket file is a filesystem artifact — remove it so the
        // control-dir does not accumulate stale sockets. On Windows the pipe is
        // reclaimed by the OS when the server handle drops.
        #[cfg(unix)]
        {
            let _ = std::fs::remove_file(&self.endpoint);
            // Best-effort: if the socket lived in a listener-created PRIVATE per-session subdir
            // (`vr-ctl-<hash>`) and that subdir is now empty, remove it too so a long-lived
            // control_dir does not accumulate one empty 0o700 dir per shim start. `remove_dir` only
            // removes an EMPTY dir, and the prefix gate keeps this off `control_dir` / any user dir;
            // errors are ignored so teardown never fails.
            if let Some(parent) = std::path::Path::new(&self.endpoint).parent() {
                let is_private_subdir = parent
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with(PRIVATE_SUBDIR_PREFIX));
                if is_private_subdir {
                    let _ = std::fs::remove_dir(parent);
                }
            }
        }
    }
}

/// Connect to a shim's advertised control endpoint. Reuses the crate's portable
/// pipe/UDS client connect (a `\\.\pipe\…` name on Windows, a UDS path on Unix)
/// — the SAME connect the `--api` attach path uses.
pub async fn connect_control_endpoint(
    endpoint: &str,
) -> TsgoApiResult<(ControlReadHalf, ControlWriteHalf)> {
    connect_attach_pipe(endpoint).await
}

/// Box a concrete read half behind the crate's boxed read-half type.
fn boxed_read<R: AsyncRead + Unpin + Send + 'static>(read: R) -> ControlReadHalf {
    Box::new(read)
}

/// Box a concrete write half behind the crate's boxed write-half type.
fn boxed_write<W: AsyncWrite + Unpin + Send + 'static>(write: W) -> ControlWriteHalf {
    Box::new(write)
}

#[cfg(test)]
#[path = "transport_tests.rs"]
mod tests;
