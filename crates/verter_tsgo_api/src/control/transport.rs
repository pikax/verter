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

/// The basename prefix of the PRIVATE per-session subdir the path-minting creates for the control
/// socket (`vr-ctl-<hash>`). ONLY the path-minting keys off it; the listener's `Drop` cleanup keys
/// off the subdir it RECORDED creating at bind (see [`PreparedParent`]), never this prefix, so a
/// pre-existing user directory that merely shares the shape is never removed.
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

/// The outcome of preparing a Unix control socket's private parent directory. It records the
/// subdir THIS bind actually created, so the listener's `Drop` removes only a directory we own —
/// never a pre-existing one that merely shares the `vr-ctl-` name.
#[cfg(unix)]
struct PreparedParent {
    /// `Some(path)` ONLY when this call's `DirBuilder::create` created the private subdir; `None`
    /// when the subdir already existed and was reused/validated (never recorded for cleanup).
    owned_private_subdir: Option<std::path::PathBuf>,
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
/// rename/swap our validated `0o700` subdir between validate and bind despite the sticky bit. A
/// grandparent (`--control-dir`) that is ITSELF a symlink is rejected up front, so a swappable
/// symlinked control_dir cannot redirect the bind to an unchecked target (a benign ANCESTOR symlink
/// like macOS `/tmp`→`/private/tmp` is still fine — only the final component's own symlink-ness is
/// rejected).
///
/// This is a MITIGATION, NOT full TOCTOU-proofing: `tokio::net::UnixListener::bind` takes a PATH
/// (there is no dir-fd bind), so a path-based bind is not atomic against an attacker who can already
/// write the grandparent and swap our validated subdir between validate and bind. The symlink
/// rejection, the euid + exact-`0o700` + real-directory gate on the subdir we own, and the (A)/(B)
/// grandparent ceiling are the accepted, standard mitigations; full open-time safety would need an
/// fd-relative bind (`openat`/`O_NOFOLLOW` against a retained dir-fd), which this path does not do —
/// `UnixListener::bind` is path-based, with no dir-fd bind.
#[cfg(unix)]
fn prepare_unix_socket_parent(endpoint: &str) -> TsgoApiResult<PreparedParent> {
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
            // Reject a control_dir that is ITSELF a symlink BEFORE creating or trusting anything
            // under it: a path-based bind is not atomic, so an attacker who can swap a symlinked
            // control_dir between validate and bind could redirect the socket to an unchecked
            // target. `symlink_metadata` does not follow the final component, so a symlinked
            // control_dir is caught here; a real dir, a not-yet-created dir, or a benign ANCESTOR
            // symlink (e.g. macOS `/tmp`→`/private/tmp`) all pass — only the final component's own
            // symlink-ness is rejected. MITIGATION, not full TOCTOU-proofing (see the fn doc).
            match std::fs::symlink_metadata(control_dir) {
                Ok(meta) if meta.file_type().is_symlink() => {
                    return Err(TsgoApiError::Transport(format!(
                        "control dir {control_dir:?} is a symlink; refusing to bind through it (a \
                         swappable symlinked control dir could redirect the control socket to an \
                         unchecked target). Pass a real directory as --control-dir."
                    )));
                }
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => {
                    return Err(TsgoApiError::Transport(format!(
                        "stat control dir {control_dir:?} failed: {e}"
                    )));
                }
            }
            std::fs::create_dir_all(control_dir).map_err(|e| {
                TsgoApiError::Transport(format!("create control dir {control_dir:?} failed: {e}"))
            })?;
            // Secure-permissions ceiling (OpenSSH-style), encoded by `grandparent_ceiling_ok`: the
            // grandparent is SAFE iff EITHER (A) owned by us with no group/other WRITE bits, OR (B)
            // sticky (`0o1000`) AND owned by us or by root. A sticky grandparent owned by another
            // non-root user is rejected — that owner could rename/swap our `0o700` subdir despite
            // the sticky bit — as is any group/other-writable non-sticky dir (fail closed).
            // `metadata` follows ANCESTOR symlinks (the immediate control_dir symlink was already
            // rejected above), so a benign symlinked ancestor still resolves to its real perms.
            // `UnixListener::bind` is path-based (there is no dir-fd bind), so this ceiling — not an
            // open-time atomic bind — is the achievable, standard mitigation for the grandparent-swap
            // TOCTOU; full open-time safety would require an fd-relative `openat`/`O_NOFOLLOW` bind.
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
    // (it may be a hostile pre-create) rather than trusted. Record whether THIS call created it,
    // so `Drop` removes only a subdir we own, never one that merely pre-existed.
    let created_private_subdir = match std::fs::DirBuilder::new().mode(0o700).create(socket_parent)
    {
        Ok(()) => true,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => false,
        Err(e) => {
            return Err(TsgoApiError::Transport(format!(
                "create private control socket parent {socket_parent:?} failed: {e}"
            )));
        }
    };

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
    Ok(PreparedParent {
        owned_private_subdir: created_private_subdir.then(|| socket_parent.to_path_buf()),
    })
}

/// Remove a stale control socket at `endpoint`, but ONLY if the path is really a socket. The
/// endpoint is inspected with `symlink_metadata` (which does NOT follow symlinks): a missing path
/// is nothing to clean; a socket is unlinked (tolerating a concurrent removal); ANYTHING else — a
/// regular file, a directory, or a SYMLINK (whose target must never be followed and deleted) —
/// fails the bind CLOSED rather than deleting a path the shim does not own. Tolerating only
/// `NotFound` on the removal keeps a permission failure or security-relevant collision a bind
/// failure, never silently swallowed.
#[cfg(unix)]
fn unlink_stale_control_socket(endpoint: &str) -> TsgoApiResult<()> {
    use std::os::unix::fs::FileTypeExt;

    let meta = match std::fs::symlink_metadata(endpoint) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => {
            return Err(TsgoApiError::Transport(format!(
                "stat stale control socket {endpoint:?} failed: {e}"
            )))
        }
    };
    if !meta.file_type().is_socket() {
        return Err(TsgoApiError::Transport(format!(
            "control endpoint {endpoint:?} already exists and is not a socket ({:?}); refusing to \
             remove it",
            meta.file_type()
        )));
    }
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

/// The Windows named-pipe security helper: an EXPLICIT owner-only DACL granting the current-user
/// SID full control and NOTHING to any broader principal (no Everyone / Authenticated Users /
/// Administrators), matching the Unix side's `0o600` socket bar. The descriptor also PINS its OWNER
/// to that same current-user SID: without an explicit owner, under an ELEVATED token the pipe owner
/// defaults to `BUILTIN\Administrators`, who hold implicit `WRITE_DAC` and could rewrite the very
/// owner-only DACL. Without the DACL a named pipe inherits the process token's ambient default DACL,
/// which grants broader access (on many hosts it grants Everyone). The descriptor is applied to EVERY
/// pipe instance — the `bind` instance AND every `accept`-provisioned next instance — and the helper
/// fails CLOSED on any Win32 error (it never falls back to a default-DACL pipe).
#[cfg(windows)]
mod owner_only_pipe_security {
    use std::ffi::c_void;

    use windows_sys::Win32::Foundation::{
        CloseHandle, GetLastError, LocalFree, ERROR_SUCCESS, FALSE, GENERIC_ALL, HANDLE, TRUE,
    };
    use windows_sys::Win32::Security::Authorization::{
        SetEntriesInAclW, EXPLICIT_ACCESS_W, NO_MULTIPLE_TRUSTEE, SET_ACCESS, TRUSTEE_IS_SID,
        TRUSTEE_IS_USER,
    };
    use windows_sys::Win32::Security::{
        GetTokenInformation, InitializeSecurityDescriptor, SetSecurityDescriptorDacl,
        SetSecurityDescriptorOwner, TokenUser, ACL, NO_INHERITANCE, PSID, SECURITY_ATTRIBUTES,
        SECURITY_DESCRIPTOR, TOKEN_QUERY, TOKEN_USER,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    use crate::error::{TsgoApiError, TsgoApiResult};

    /// `SECURITY_DESCRIPTOR_REVISION` — the frozen Win32 security-descriptor ABI revision (`1`).
    /// windows-sys names it only under `Win32_System_SystemServices`, a module this crate
    /// otherwise does not pull, so the stable ABI constant is named locally.
    const SECURITY_DESCRIPTOR_REVISION: u32 = 1;

    /// An owner-only pipe security descriptor: it owns the heap-stable `SECURITY_DESCRIPTOR` and the
    /// `SetEntriesInAclW`-allocated DACL, keeps both alive across the pipe-create call, and frees the
    /// DACL on drop (the kernel copies the security info into the pipe object at creation time).
    pub(super) struct OwnerOnlyPipeSecurity {
        /// Boxed for a stable address — `SECURITY_ATTRIBUTES.lpSecurityDescriptor` points at it.
        descriptor: Box<SECURITY_DESCRIPTOR>,
        /// The DACL allocated by `SetEntriesInAclW` (via `LocalAlloc`); `LocalFree`d on drop.
        dacl: *mut ACL,
        /// Backs the current-user SID the DACL AND the descriptor OWNER reference; retained until
        /// the kernel copies the security descriptor into the pipe object at creation.
        _token_user: Vec<u8>,
    }

    impl OwnerOnlyPipeSecurity {
        /// Build the current-user-only security descriptor, or fail closed on any Win32 error.
        pub(super) fn build() -> TsgoApiResult<Self> {
            // SAFETY: each Win32 call below is used per its contract and every fallible result is
            // checked (the code fails closed, never proceeding on an error). Raw pointers stay
            // valid: `token` is closed by `TokenHandle`'s drop; the retained `token_user` buffer is
            // LOAD-BEARING for BOTH the DACL and the OWNER — the SID aliases it and must outlive
            // `SetEntriesInAclW` copying the SID into `dacl` AND the kernel copying the SD owner (set
            // via `SetSecurityDescriptorOwner`, which references the SID DIRECTLY) at pipe creation.
            // `token_user`, `dacl`, and the boxed descriptor are all retained on the returned value
            // until then.
            unsafe {
                let mut token: HANDLE = std::ptr::null_mut();
                if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == FALSE {
                    return Err(win32_last_error("OpenProcessToken"));
                }
                let _token = TokenHandle(token);

                // Size the TOKEN_USER, then fetch it.
                let mut needed: u32 = 0;
                GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut needed);
                if needed == 0 {
                    return Err(win32_last_error("GetTokenInformation (size probe)"));
                }
                let mut token_user = vec![0u8; needed as usize];
                if GetTokenInformation(
                    token,
                    TokenUser,
                    token_user.as_mut_ptr().cast::<c_void>(),
                    needed,
                    &mut needed,
                ) == FALSE
                {
                    return Err(win32_last_error("GetTokenInformation"));
                }
                // SAFETY: `token_user` is a `Vec<u8>` (align-1) buffer, so forming a `&TOKEN_USER`
                // place to project `.User.Sid` would read the `PSID` through an under-aligned
                // reference (UB by the letter). `addr_of!` computes the field address by offset
                // WITHOUT forming a reference, and `read_unaligned` reads the `PSID` value from it;
                // the SID bytes it points at stay owned by the retained `token_user`. The pointer
                // VALUE is identical to a direct field read, so the Win32 APIs receive the same SID.
                let tu = token_user.as_ptr().cast::<TOKEN_USER>();
                let sid: PSID = core::ptr::read_unaligned(core::ptr::addr_of!((*tu).User.Sid));
                if sid.is_null() {
                    return Err(TsgoApiError::Transport(
                        "control named pipe owner-only DACL: token user SID is null".to_string(),
                    ));
                }

                // One ACE: the current user gets full control; no other principal is named.
                let mut ea: EXPLICIT_ACCESS_W = std::mem::zeroed();
                ea.grfAccessPermissions = GENERIC_ALL;
                ea.grfAccessMode = SET_ACCESS;
                ea.grfInheritance = NO_INHERITANCE;
                ea.Trustee.MultipleTrusteeOperation = NO_MULTIPLE_TRUSTEE;
                ea.Trustee.TrusteeForm = TRUSTEE_IS_SID;
                ea.Trustee.TrusteeType = TRUSTEE_IS_USER;
                ea.Trustee.ptstrName = sid.cast::<u16>();

                let mut dacl: *mut ACL = std::ptr::null_mut();
                let rc = SetEntriesInAclW(1, &ea, std::ptr::null(), &mut dacl);
                if rc != ERROR_SUCCESS || dacl.is_null() {
                    return Err(win32_code("SetEntriesInAclW", rc));
                }

                // From here a failure must free the just-allocated DACL before returning.
                let mut descriptor: Box<SECURITY_DESCRIPTOR> = Box::new(std::mem::zeroed());
                let sd_ptr = std::ptr::addr_of_mut!(*descriptor).cast::<c_void>();
                if InitializeSecurityDescriptor(sd_ptr, SECURITY_DESCRIPTOR_REVISION) == FALSE {
                    let _ = LocalFree(dacl.cast::<c_void>());
                    return Err(win32_last_error("InitializeSecurityDescriptor"));
                }
                if SetSecurityDescriptorDacl(sd_ptr, TRUE, dacl, FALSE) == FALSE {
                    let _ = LocalFree(dacl.cast::<c_void>());
                    return Err(win32_last_error("SetSecurityDescriptorDacl"));
                }
                // PIN the OWNER to the current-user SID (`FALSE` = explicitly set, NOT defaulted).
                // Without it, under an ELEVATED token the pipe owner defaults to
                // `BUILTIN\Administrators`, who hold implicit `WRITE_DAC` and could rewrite the
                // owner-only DACL — breaking the current-user-only / 0o600 parity. The owner
                // references `sid` DIRECTLY (aliasing the retained `token_user`) until the kernel
                // copies the descriptor at pipe creation.
                if SetSecurityDescriptorOwner(sd_ptr, sid, FALSE) == FALSE {
                    let _ = LocalFree(dacl.cast::<c_void>());
                    return Err(win32_last_error("SetSecurityDescriptorOwner"));
                }

                Ok(Self {
                    descriptor,
                    dacl,
                    _token_user: token_user,
                })
            }
        }

        /// A `SECURITY_ATTRIBUTES` referencing this descriptor. The returned value borrows `self`'s
        /// heap-stable descriptor, so it is valid only while `self` is alive.
        pub(super) fn security_attributes(&self) -> SECURITY_ATTRIBUTES {
            SECURITY_ATTRIBUTES {
                nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: std::ptr::addr_of!(*self.descriptor) as *mut c_void,
                bInheritHandle: FALSE,
            }
        }
    }

    impl Drop for OwnerOnlyPipeSecurity {
        fn drop(&mut self) {
            if !self.dacl.is_null() {
                // SAFETY: `dacl` came from `SetEntriesInAclW` (a `LocalAlloc` allocation) and is
                // freed exactly once here; the kernel already copied the descriptor at pipe
                // creation, so releasing our copy is sound.
                unsafe {
                    let _ = LocalFree(self.dacl.cast::<c_void>());
                }
                self.dacl = std::ptr::null_mut();
            }
        }
    }

    /// Closes an owned process-token handle on drop.
    struct TokenHandle(HANDLE);

    impl Drop for TokenHandle {
        fn drop(&mut self) {
            // SAFETY: `self.0` is a token handle from `OpenProcessToken` we exclusively own.
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }

    fn win32_last_error(op: &str) -> TsgoApiError {
        // SAFETY: `GetLastError` reads the calling thread's error state; it is always sound.
        let code = unsafe { GetLastError() };
        TsgoApiError::Transport(format!(
            "control named pipe owner-only DACL: {op} failed (GetLastError {code})"
        ))
    }

    fn win32_code(op: &str, code: u32) -> TsgoApiError {
        TsgoApiError::Transport(format!(
            "control named pipe owner-only DACL: {op} failed (code {code})"
        ))
    }
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
    /// The private per-session subdir THIS listener created at bind (if any). `Drop` removes only
    /// this recorded, owned path — never a pre-existing directory that merely matches the name.
    #[cfg(unix)]
    owned_private_subdir: Option<std::path::PathBuf>,
}

impl ControlListener {
    /// Bind the control endpoint. Must be called within a tokio runtime (the
    /// listener registers with the reactor). On Unix a stale socket file at the
    /// path is removed first (best-effort) so a re-bind after an unclean exit
    /// still succeeds.
    pub fn bind(endpoint: &str) -> TsgoApiResult<Self> {
        #[cfg(windows)]
        {
            use std::ffi::c_void;
            use tokio::net::windows::named_pipe::ServerOptions;
            // Apply an EXPLICIT owner-only DACL (current-user SID only) so the pipe matches the
            // Unix `0o600` socket bar instead of inheriting the token's broader default DACL.
            let security = owner_only_pipe_security::OwnerOnlyPipeSecurity::build()?;
            let mut sa = security.security_attributes();
            // SAFETY: `sa` points at `security`'s heap-stable descriptor, which stays alive across
            // this call; the descriptor names only the current user (owner-only, the 0o600 parity).
            let server = unsafe {
                ServerOptions::new()
                    .first_pipe_instance(true)
                    .create_with_security_attributes_raw(
                        endpoint,
                        std::ptr::addr_of_mut!(sa).cast::<c_void>(),
                    )
            }
            .map_err(|e| {
                TsgoApiError::Transport(format!("bind control named pipe {endpoint:?} failed: {e}"))
            })?;
            // The kernel copied the descriptor at creation; release the ACL now.
            drop(security);
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
            let prepared = prepare_unix_socket_parent(endpoint)?;
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
                owned_private_subdir: prepared.owned_private_subdir,
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
            // Provision the next instance for a subsequent client, with the SAME explicit
            // owner-only DACL the bind instance carries (every instance matches the 0o600 bar).
            let next = {
                use std::ffi::c_void;
                let security = owner_only_pipe_security::OwnerOnlyPipeSecurity::build()?;
                let mut sa = security.security_attributes();
                // SAFETY: as in `bind` — `sa` references `security`'s heap-stable owner-only
                // descriptor, which outlives this create call.
                let created = unsafe {
                    ServerOptions::new().create_with_security_attributes_raw(
                        &self.endpoint,
                        std::ptr::addr_of_mut!(sa).cast::<c_void>(),
                    )
                }
                .map_err(|e| {
                    TsgoApiError::Transport(format!(
                        "provision next control pipe instance failed: {e}"
                    ))
                })?;
                drop(security);
                created
            };
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
            // Best-effort: remove ONLY the PRIVATE per-session subdir THIS listener created
            // (recorded at bind), and only if it is now empty, so a long-lived control_dir does not
            // accumulate one empty 0o700 dir per shim start. A subdir that already existed and was
            // merely reused is left in place — even if its name matches the `vr-ctl-` shape — so a
            // pre-existing user directory is never deleted. `remove_dir` removes only an EMPTY dir;
            // errors are ignored so teardown never fails.
            if let Some(subdir) = &self.owned_private_subdir {
                let _ = std::fs::remove_dir(subdir);
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
