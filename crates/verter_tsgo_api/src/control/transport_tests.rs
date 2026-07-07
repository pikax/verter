//! Tests for the control endpoint: a REAL platform pipe/socket round-trip
//! through the shim-side [`ControlListener`] + the client-side connect, plus
//! the portable endpoint-path minting and a discriminating connect failure.

use super::*;
use crate::jsonrpc::framing::{encode_message, MessageFramer};
use crate::jsonrpc::JsonRpcConnection;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn unique_disamb() -> String {
    format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    )
}

#[test]
fn endpoint_path_is_portable_and_platform_selected() {
    let dir = std::env::temp_dir();
    let endpoint = control_endpoint_path(&dir, r"C:\weird:session", 4242, "abc");
    #[cfg(windows)]
    {
        assert!(
            endpoint.starts_with(r"\\.\pipe\"),
            "a Windows control endpoint is a named pipe: {endpoint:?}"
        );
        // The name segment (after the pipe prefix) carries no NTFS-illegal chars.
        let name = endpoint.strip_prefix(r"\\.\pipe\").unwrap();
        for bad in ['<', '>', ':', '"', '|', '?', '*'] {
            assert!(!name.contains(bad), "pipe name must not contain {bad:?}");
        }
    }
    #[cfg(unix)]
    {
        assert!(
            endpoint.ends_with(".sock"),
            "a Unix control endpoint is a UDS path: {endpoint:?}"
        );
        assert!(
            endpoint.len() <= 108,
            "the UDS path must fit the sockaddr_un budget: {endpoint:?}"
        );
        // The socket's IMMEDIATE parent is a PRIVATE per-session subdir (created 0o700 at
        // bind), never control_dir directly — so control_dir may stay a shared dir.
        assert!(
            endpoint.contains("/vr-ctl-"),
            "the socket nests in a private per-session subdir: {endpoint:?}"
        );
    }
    // The pid keys the endpoint (Windows: literal in the pipe name; Unix: via the subdir
    // hash) — a different pid mints a DISTINCT endpoint.
    let other_pid = control_endpoint_path(&dir, r"C:\weird:session", 9999, "abc");
    assert_ne!(endpoint, other_pid, "the pid keys the endpoint");
}

/// A REAL control endpoint accepts the client connect and a JSON-RPC round-trip
/// crosses the actual OS transport — the net-new SERVER (listener) side proven
/// end-to-end (not just the codec).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn control_endpoint_round_trips_over_real_transport() {
    // The control_dir. The bind nests the socket in a PRIVATE per-session subdir it creates
    // 0o700 under this dir (control_dir itself only needs to exist + be traversable). On
    // Windows `dir` is ignored (the endpoint is a named pipe).
    let dir = std::env::temp_dir().join(format!("vr-rt-{}", unique_disamb()));
    let endpoint = control_endpoint_path(&dir, "rt", std::process::id(), &unique_disamb());
    let mut listener = ControlListener::bind(&endpoint).expect("bind control endpoint");
    let server_endpoint = listener.endpoint().to_string();

    // Server: accept one control connection, echo a framed request as `{ ok: method }`.
    let server_task = tokio::spawn(async move {
        let (mut read, mut write) = listener.accept().await.expect("accept control connection");
        let mut framer = MessageFramer::new();
        let mut chunk = [0u8; 4096];
        loop {
            let n = match read.read(&mut chunk).await {
                Ok(0) | Err(_) => return,
                Ok(n) => n,
            };
            framer.push(&chunk[..n]);
            while let Ok(Some(msg)) = framer.next_message() {
                if let (Some(id), Some(method)) = (
                    msg.get("id").cloned(),
                    msg.get("method").and_then(|m| m.as_str()),
                ) {
                    let reply = serde_json::json!({
                        "jsonrpc": "2.0", "id": id, "result": { "ok": method }
                    });
                    let _ = write.write_all(&encode_message(&reply)).await;
                    let _ = write.flush().await;
                    return;
                }
            }
        }
    });

    let (read, write) = connect_control_endpoint(&server_endpoint)
        .await
        .expect("client connect");
    let conn = JsonRpcConnection::connect(read, write);
    let result = conn
        .request("verter/hello", serde_json::json!({ "probe": true }))
        .await
        .expect("round-trip over the control endpoint");
    assert_eq!(result["ok"], serde_json::json!("verter/hello"));
    conn.close().await.unwrap();
    let _ = server_task.await;
    let _ = std::fs::remove_dir_all(&dir);
}

/// I2 — the Unix bind creates + control-binds the socket's PRIVATE parent directory
/// (`0o700`, a real dir, euid-owned, no group/other bits) BEFORE binding, and locks the
/// bound socket down to `0o600`. Its grandparent (here the temp base) is created with default
/// perms. UNIX-ONLY (POSIX permissions + UDS); cfg-compiled-out on Windows.
///
/// RED before the fix: `bind` did a best-effort `remove_file` then `UnixListener::bind`
/// with NO parent-dir creation, so binding at a path whose parent directory does not exist
/// fails (`ENOENT`). GREEN: `prepare_unix_socket_parent` creates the parent `0o700`, the
/// bind succeeds, the socket is chmod'd `0o600`, and a real round-trip crosses it.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bind_creates_private_parent_dir_before_unix_socket_bind() {
    use std::os::unix::fs::PermissionsExt;

    // An endpoint whose PARENT directory does NOT exist yet.
    let base = std::env::temp_dir().join(format!("vr-i2-{}", unique_disamb()));
    let parent = base.join("private");
    let endpoint = parent.join("ctl.sock");
    let endpoint_str = endpoint.to_string_lossy().into_owned();
    assert!(
        !parent.exists(),
        "the parent dir must not exist before bind"
    );

    // Post-fix: bind creates the private parent, binds, and chmods the socket 0o600.
    // Pre-fix: `UnixListener::bind` fails because the parent directory is missing.
    let mut listener = ControlListener::bind(&endpoint_str)
        .expect("bind must create the private parent dir + bind the socket");
    let server_endpoint = listener.endpoint().to_string();

    // The parent exists, is a REAL directory, and is private 0o700 (no group/other bits).
    let parent_meta = std::fs::symlink_metadata(&parent).expect("parent stat");
    assert!(
        parent_meta.file_type().is_dir(),
        "the socket parent must be a real directory, not a symlink"
    );
    assert_eq!(
        parent_meta.permissions().mode() & 0o777,
        0o700,
        "the socket parent must be private 0o700"
    );

    // The bound socket is owner-only 0o600.
    let sock_meta = std::fs::symlink_metadata(&endpoint).expect("socket stat");
    assert_eq!(
        sock_meta.permissions().mode() & 0o777,
        0o600,
        "the control socket must be locked down to 0o600"
    );

    // A real round-trip proves the listener is FUNCTIONAL, not merely created.
    let server = tokio::spawn(async move {
        let (mut read, mut write) = listener.accept().await.expect("accept");
        let mut buf = [0u8; 64];
        let n = read.read(&mut buf).await.unwrap_or(0);
        let _ = write.write_all(&buf[..n]).await;
        let _ = write.flush().await;
    });
    let (mut read, mut write) = connect_control_endpoint(&server_endpoint)
        .await
        .expect("connect the created control socket");
    write.write_all(b"ping").await.unwrap();
    write.flush().await.unwrap();
    let mut buf = [0u8; 64];
    let n = read.read(&mut buf).await.unwrap();
    assert_eq!(&buf[..n], b"ping", "the created socket must round-trip");
    let _ = server.await;

    let _ = std::fs::remove_dir_all(&base);
}

/// F3 — a legitimate PRE-EXISTING `--control-dir` at a shared `0o755` mode STILL binds: the
/// socket nests in a PRIVATE per-session subdir the bind creates `0o700`, while control_dir
/// itself is left UNTOUCHED (never tightened to 0o700 — the user may legitimately share it).
/// UNIX-ONLY.
///
/// RED before the fix: `prepare_unix_socket_parent` created + validated `control_dir` itself
/// (the socket's direct parent) at `0o700`, so a pre-existing `0o755` control_dir failed the
/// no-group/other-bits check and the bind failed CLOSED. GREEN: the private subdir carries the
/// privacy guarantee, so a shared control_dir binds and stays 0o755.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn legitimate_shared_control_dir_still_binds_and_stays_untightened() {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};

    // A pre-existing, legitimately-shared control_dir at 0o755 (forced, umask-independent).
    let control_dir = std::env::temp_dir().join(format!("vr-shared-{}", unique_disamb()));
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o755)
        .create(&control_dir)
        .expect("create shared control dir");
    std::fs::set_permissions(&control_dir, std::fs::Permissions::from_mode(0o755))
        .expect("force control_dir to a shared 0o755");

    // The socket's immediate parent is a private per-session subdir UNDER the shared dir.
    let subdir = control_dir.join("priv");
    let endpoint = subdir.join("ctl.sock");
    let endpoint_str = endpoint.to_string_lossy().into_owned();

    let mut listener = ControlListener::bind(&endpoint_str)
        .expect("a shared 0o755 control_dir must still bind (socket nests in a private subdir)");
    let server_endpoint = listener.endpoint().to_string();

    // control_dir was NOT tightened — still 0o755.
    assert_eq!(
        std::fs::symlink_metadata(&control_dir)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o755,
        "control_dir must be left untouched (never forced to 0o700)"
    );
    // The socket's private per-session subdir IS 0o700, and the socket is 0o600.
    assert_eq!(
        std::fs::symlink_metadata(&subdir)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700,
        "the per-session socket subdir must be private 0o700"
    );
    assert_eq!(
        std::fs::symlink_metadata(&endpoint)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600,
        "the control socket must be locked down to 0o600"
    );

    // A real round-trip proves the listener is FUNCTIONAL, not merely created.
    let server = tokio::spawn(async move {
        let (mut read, mut write) = listener.accept().await.expect("accept");
        let mut buf = [0u8; 64];
        let n = read.read(&mut buf).await.unwrap_or(0);
        let _ = write.write_all(&buf[..n]).await;
        let _ = write.flush().await;
    });
    let (mut read, mut write) = connect_control_endpoint(&server_endpoint)
        .await
        .expect("connect the created control socket");
    write.write_all(b"ping").await.unwrap();
    write.flush().await.unwrap();
    let mut buf = [0u8; 64];
    let n = read.read(&mut buf).await.unwrap();
    assert_eq!(&buf[..n], b"ping", "the created socket must round-trip");
    let _ = server.await;

    let _ = std::fs::remove_dir_all(&control_dir);
}

/// F3 — a HOSTILE pre-existing socket-parent subdir fails the bind CLOSED. The bind does NOT
/// trust a pre-created parent: it re-validates the private perms (and, not exercised here, the
/// euid ownership — staging a foreign-owned dir needs root). A same-owner subdir left with
/// group/other bits is rejected. UNIX-ONLY.
///
/// RED against a bind that trusts a pre-existing parent (or only creates-if-missing without
/// re-validating): it would bind under the loose-perm parent. GREEN: the mode re-validation
/// rejects it.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hostile_preexisting_socket_parent_fails_closed() {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};

    let control_dir = std::env::temp_dir().join(format!("vr-hostile-{}", unique_disamb()));
    let subdir = control_dir.join("priv");
    // Pre-create the socket's private subdir with LOOSE (group/other-accessible) perms —
    // exactly what a hostile pre-create would leave behind.
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o755)
        .create(&subdir)
        .expect("pre-create the loose-perm subdir");
    std::fs::set_permissions(&subdir, std::fs::Permissions::from_mode(0o755))
        .expect("force the hostile subdir to loose 0o755");
    let endpoint = subdir.join("ctl.sock");
    let endpoint_str = endpoint.to_string_lossy().into_owned();

    // `ControlListener` is not `Debug`, so match rather than format the `Ok` arm.
    match ControlListener::bind(&endpoint_str) {
        Err(crate::error::TsgoApiError::Transport(_)) => {}
        Ok(_) => {
            panic!("a pre-existing socket parent with group/other perms must fail the bind CLOSED")
        }
        Err(other) => panic!("expected a Transport bind failure, got {other:?}"),
    }
    // No socket was bound under the rejected parent.
    assert!(
        !endpoint.exists(),
        "no socket must be bound under a rejected parent"
    );

    let _ = std::fs::remove_dir_all(&control_dir);
}

/// The grandparent secure-permissions ceiling (`grandparent_ceiling_ok`) encodes the (A)/(B) rule
/// exactly. It is a PURE predicate over `(owner uid, st_mode, euid)`, so this test runs — and
/// discriminates — on EVERY platform without needing root or a real other-user-owned directory.
///
/// The `sticky + owned-by-another-non-root-user => REJECTED` case is the RED->GREEN discriminator:
/// it FAILS against the superseded `sticky`-alone ceiling (which accepted any sticky dir regardless
/// of its owner) and PASSES under the (A)/(B) ceiling. `euid` is fixed at 1000, root at 0.
#[test]
fn grandparent_ceiling_ok_encodes_owned_or_sticky_trusted_owner() {
    const EUID: u32 = 1000;
    const ROOT: u32 = 0;
    const OTHER: u32 = 1234;

    // (B) sticky AND owned by a trusted owner (us or root) => ACCEPTED.
    assert!(
        grandparent_ceiling_ok(EUID, 0o1777, EUID),
        "sticky + owned-by-euid must be ACCEPTED"
    );
    assert!(
        grandparent_ceiling_ok(ROOT, 0o1777, EUID),
        "sticky + owned-by-root (the /tmp 0o1777 case) must be ACCEPTED"
    );

    // (B) violated: sticky but owned by ANOTHER non-root user => REJECTED. THE discriminator — the
    // superseded `sticky`-alone ceiling accepted this (its owner can still swap our subdir under
    // the sticky bit); the (A)/(B) ceiling fails it closed.
    assert!(
        !grandparent_ceiling_ok(OTHER, 0o1777, EUID),
        "sticky + owned-by-another-non-root-user must be REJECTED (the sticky-alone discriminator)"
    );
    assert!(
        !grandparent_ceiling_ok(OTHER, 0o1755, EUID),
        "sticky + owned-by-another-non-root-user is REJECTED even with no group/other WRITE bits"
    );

    // (A) owned by the euid with no group/other WRITE bits => ACCEPTED (sticky bit irrelevant).
    assert!(
        grandparent_ceiling_ok(EUID, 0o700, EUID),
        "non-sticky + owned-by-euid + 0o700 must be ACCEPTED"
    );
    assert!(
        grandparent_ceiling_ok(EUID, 0o755, EUID),
        "non-sticky + owned-by-euid + 0o755 (no group/other WRITE) must be ACCEPTED"
    );

    // (A) violated: owned by the euid but group/other-writable and non-sticky => REJECTED.
    assert!(
        !grandparent_ceiling_ok(EUID, 0o777, EUID),
        "non-sticky + owned-by-euid + group/other write (0o777) must be REJECTED"
    );
    assert!(
        !grandparent_ceiling_ok(EUID, 0o775, EUID),
        "non-sticky + owned-by-euid + group write (0o775) must be REJECTED"
    );
    assert!(
        !grandparent_ceiling_ok(EUID, 0o702, EUID),
        "non-sticky + owned-by-euid + other write (0o702) must be REJECTED"
    );

    // Neither (A) nor (B): non-sticky and not owned by the euid => REJECTED (fail closed), whether
    // the owner is another non-root user or even root (a non-sticky root-owned dir is not ours).
    assert!(
        !grandparent_ceiling_ok(OTHER, 0o755, EUID),
        "non-sticky + owned-by-another-user must be REJECTED"
    );
    assert!(
        !grandparent_ceiling_ok(ROOT, 0o755, EUID),
        "non-sticky + owned-by-root (not ours, no sticky bit) must be REJECTED (fail closed)"
    );
}

/// A group/other-writable, NON-sticky control_dir (the socket's GRANDPARENT) is rejected: such a
/// dir lets another local user rename/swap our validated `0o700` subdir between validate and bind,
/// so the bind fails CLOSED under the standard secure-permissions ceiling. UNIX-ONLY.
///
/// RED against a bind that creates/traverses control_dir with no permission ceiling: it binds
/// under the world-writable grandparent. GREEN: the sticky/owned-safe ceiling rejects it.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn writable_nonsticky_control_dir_grandparent_fails_closed() {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};

    // A world-writable, NON-sticky control_dir (forced 0o777, umask-independent).
    let control_dir = std::env::temp_dir().join(format!("vr-gpw-{}", unique_disamb()));
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o777)
        .create(&control_dir)
        .expect("create the world-writable control dir");
    std::fs::set_permissions(&control_dir, std::fs::Permissions::from_mode(0o777))
        .expect("force control_dir world-writable non-sticky 0o777");

    let subdir = control_dir.join("priv");
    let endpoint = subdir.join("ctl.sock");
    let endpoint_str = endpoint.to_string_lossy().into_owned();

    match ControlListener::bind(&endpoint_str) {
        Err(crate::error::TsgoApiError::Transport(msg)) => {
            assert!(
                msg.contains("unsafe") || msg.contains("sticky"),
                "the rejection must be the secure-permissions ceiling, not an unrelated error: {msg}"
            );
        }
        Ok(_) => panic!("a world-writable non-sticky control_dir must fail the bind CLOSED"),
        Err(other) => panic!("expected a Transport bind failure, got {other:?}"),
    }
    assert!(
        !endpoint.exists(),
        "no socket must be bound under a rejected grandparent"
    );
    let _ = std::fs::remove_dir_all(&control_dir);
}

/// A sticky, world-writable control_dir (like `/tmp` at `0o1777`) is ACCEPTED: the sticky bit
/// means only an entry's owner may rename/delete it, so our `0o700` subdir cannot be swapped. The
/// owned-safe (`0o755`) accept case is covered by `legitimate_shared_control_dir_still_binds…`.
/// UNIX-ONLY.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sticky_world_writable_control_dir_grandparent_is_accepted() {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};

    let control_dir = std::env::temp_dir().join(format!("vr-gps-{}", unique_disamb()));
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o1777)
        .create(&control_dir)
        .expect("create the sticky control dir");
    std::fs::set_permissions(&control_dir, std::fs::Permissions::from_mode(0o1777))
        .expect("force control_dir sticky world-writable 0o1777");

    let subdir = control_dir.join("priv");
    let endpoint = subdir.join("ctl.sock");
    let endpoint_str = endpoint.to_string_lossy().into_owned();

    let mut listener = ControlListener::bind(&endpoint_str)
        .expect("a sticky (0o1777) control_dir must bind — the sticky bit protects the subdir");
    let server_endpoint = listener.endpoint().to_string();

    // A real round-trip proves the listener is FUNCTIONAL, not merely created.
    let server = tokio::spawn(async move {
        let (mut read, mut write) = listener.accept().await.expect("accept");
        let mut buf = [0u8; 64];
        let n = read.read(&mut buf).await.unwrap_or(0);
        let _ = write.write_all(&buf[..n]).await;
        let _ = write.flush().await;
    });
    let (mut read, mut write) = connect_control_endpoint(&server_endpoint)
        .await
        .expect("connect the created control socket");
    write.write_all(b"ping").await.unwrap();
    write.flush().await.unwrap();
    let mut buf = [0u8; 64];
    let n = read.read(&mut buf).await.unwrap();
    assert_eq!(&buf[..n], b"ping", "the created socket must round-trip");
    let _ = server.await;
    let _ = std::fs::remove_dir_all(&control_dir);
}

/// An over-budget control socket path fails closed on the `sockaddr_un` budget with a CLEAR error
/// — covering the temp-dir fallback overflow class (a very long TMPDIR would blow the budget).
/// UNIX-ONLY.
///
/// RED against a bind that hands the overlong path straight to `UnixListener::bind`: the failure is
/// an opaque OS error (or a dir is created under the giant path first), never a clear budget
/// message. GREEN: the pre-bind budget gate rejects it up front with an actionable error.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn overlong_unix_socket_path_fails_closed_on_the_budget() {
    // A socket path deliberately well past the ~108-byte sockaddr_un budget.
    let overlong = format!("/tmp/{}/ctl.sock", "d".repeat(140));
    assert!(
        overlong.len() > 108,
        "the probe path must exceed the budget"
    );
    match ControlListener::bind(&overlong) {
        Err(crate::error::TsgoApiError::Transport(msg)) => {
            assert!(
                msg.contains("sockaddr_un") || msg.contains("budget"),
                "an over-budget path must be a clear budget error, not an opaque one: {msg}"
            );
        }
        Ok(_) => panic!("an over-budget socket path must fail closed"),
        Err(other) => panic!("expected a Transport budget error, got {other:?}"),
    }
    // The budget gate runs BEFORE any directory is created, so nothing was left on disk.
    assert!(
        !std::path::Path::new(&overlong).parent().unwrap().exists(),
        "the budget gate must reject before creating any directory"
    );
}

/// A pre-existing same-user socket-parent subdir at an owner-only but non-`0o700` mode (`0o600` —
/// no execute, so NOT a traversable private dir) is rejected: `mode & 0o077 == 0` alone would
/// wrongly accept it, but the promised invariant is EXACTLY `0o700`. UNIX-ONLY.
///
/// RED against the `mode & 0o077` check: `0o600` passes it, so the bind proceeds and fails later
/// with an opaque permission error (never the private-mode message). GREEN: the exact-`0o700`
/// check rejects it up front.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn preexisting_owner_only_non_0o700_subdir_fails_closed() {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};

    let control_dir = std::env::temp_dir().join(format!("vr-mode-{}", unique_disamb()));
    let subdir = control_dir.join("priv");
    // An owned-safe grandparent (0o700), then a same-user pre-created subdir at 0o600.
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&control_dir)
        .expect("create the owned-safe control dir");
    std::fs::DirBuilder::new()
        .mode(0o600)
        .create(&subdir)
        .expect("pre-create the 0o600 subdir");
    std::fs::set_permissions(&subdir, std::fs::Permissions::from_mode(0o600))
        .expect("force the subdir to owner-only 0o600");
    let endpoint = subdir.join("ctl.sock");
    let endpoint_str = endpoint.to_string_lossy().into_owned();

    match ControlListener::bind(&endpoint_str) {
        Err(crate::error::TsgoApiError::Transport(msg)) => {
            assert!(
                msg.contains("0o700"),
                "the rejection must be the exact-0o700 private-mode check, not an opaque error: {msg}"
            );
        }
        Ok(_) => panic!("a 0o600 subdir must fail closed (it is not the promised 0o700)"),
        Err(other) => panic!("expected a Transport error, got {other:?}"),
    }
    assert!(
        !endpoint.exists(),
        "no socket must be bound under a rejected parent"
    );
    // Restore owner-traversable perms so the temp-dir cleanup can recurse.
    let _ = std::fs::set_permissions(&subdir, std::fs::Permissions::from_mode(0o700));
    let _ = std::fs::remove_dir_all(&control_dir);
}

/// The listener's `Drop` removes not only the socket file but the now-empty PRIVATE per-session
/// subdir it created (`vr-ctl-<hash>`), so a long-lived control_dir does not accumulate one empty
/// `0o700` dir per shim start. UNIX-ONLY.
///
/// RED against a `Drop` that removes only the socket: the empty private subdir persists after the
/// listener drops. GREEN: the empty subdir is removed too.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn listener_drop_removes_the_empty_private_subdir() {
    let control_dir = std::env::temp_dir().join(format!("vr-drop-{}", unique_disamb()));
    std::fs::create_dir_all(&control_dir).expect("create control dir");
    // Mint a REAL endpoint — it nests in a `vr-ctl-<hash>` private subdir.
    let endpoint =
        control_endpoint_path(&control_dir, "drop", std::process::id(), &unique_disamb());
    let subdir = std::path::Path::new(&endpoint)
        .parent()
        .expect("the socket has a private-subdir parent")
        .to_path_buf();
    assert!(
        subdir
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with("vr-ctl-")),
        "the minted endpoint must nest in a vr-ctl-<hash> private subdir: {endpoint:?}"
    );

    {
        let _listener = ControlListener::bind(&endpoint).expect("bind the control endpoint");
        assert!(
            subdir.exists(),
            "the private subdir exists while the listener is bound"
        );
    } // the listener drops here

    assert!(
        !std::path::Path::new(&endpoint).exists(),
        "the socket file is removed on listener drop"
    );
    assert!(
        !subdir.exists(),
        "the empty private per-session subdir is removed on listener drop"
    );
    let _ = std::fs::remove_dir_all(&control_dir);
}

/// F4 — a chmod failure AFTER a successful bind must unlink the just-bound socket before
/// returning `Err`: no `ControlListener` is constructed yet, so its `Drop` cleanup never runs
/// and the socket would otherwise leak. Forcing `set_permissions` to fail after a successful
/// `UnixListener::bind` is not portably inducible (it needs the socket to become unwritable
/// mid-bind), so this is a source-structure guard on the cleanup ordering. Reads the raw
/// source, so it runs (and discriminates) on every platform.
#[test]
fn chmod_failure_after_bind_unlinks_the_socket() {
    let src = include_str!("transport.rs");
    let chmod = src
        .find("if let Err(e) = set_control_socket_permissions(endpoint)")
        .expect("the bind chmods the bound socket and handles the failure");
    let after = &src[chmod..];
    let unlink = after
        .find("std::fs::remove_file(endpoint)")
        .expect("the chmod-failure branch unlinks the just-bound socket");
    let return_err = after
        .find("return Err(e)")
        .expect("the chmod-failure branch returns the error");
    assert!(
        unlink < return_err,
        "the chmod-failure branch must remove_file the socket BEFORE returning Err (no leak); \
         unlink@{unlink} return@{return_err}"
    );
}

/// A connect to a non-existent control endpoint is a typed error, never a panic
/// or a false success (fail closed).
#[tokio::test]
async fn connect_to_missing_control_endpoint_is_a_typed_error() {
    #[cfg(windows)]
    let bogus = format!(r"\\.\pipe\verter-relay-ctl-missing-{}", std::process::id());
    #[cfg(unix)]
    let bogus = format!("/tmp/verter-relay-ctl-missing-{}.sock", std::process::id());

    match connect_control_endpoint(&bogus).await {
        Ok(_) => panic!("connecting to a missing control endpoint must not succeed"),
        Err(crate::error::TsgoApiError::Transport(_)) => {}
        Err(other) => panic!("a missing endpoint must be a typed Transport error, got {other:?}"),
    }
}
