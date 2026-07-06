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
        // The `sockaddr_un.sun_path` budget is ~104–108 bytes across Unixes.
        // Stay well under it: try `control_dir`, else a short hashed temp name.
        const UDS_MAX: usize = 100;
        let name = format!("verter-relay-ctl-{session}-{pid}-{disamb}.sock");
        let in_dir = control_dir.join(&name);
        if in_dir.as_os_str().len() <= UDS_MAX {
            return in_dir.to_string_lossy().into_owned();
        }
        let hashed = super::advertisement::stable_hash_str(&format!("{session}-{pid}-{disamb}"));
        let short = std::env::temp_dir().join(format!("vr-ctl-{hashed:016x}.sock"));
        short.to_string_lossy().into_owned()
    }
    #[cfg(not(any(windows, unix)))]
    {
        let _ = (control_dir, pid);
        format!("verter-relay-ctl-{session}-{disamb}")
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
            let _ = std::fs::remove_file(endpoint);
            let listener = tokio::net::UnixListener::bind(endpoint).map_err(|e| {
                TsgoApiError::Transport(format!(
                    "bind control unix socket {endpoint:?} failed: {e}"
                ))
            })?;
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
