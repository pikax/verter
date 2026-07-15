//! The real stdio-pipe duplex transport backed by a spawned tsgo process.
//!
//! Spawns `tsgo --api --cwd <dir> [--callbacks=…]` with piped stdin/stdout and
//! drives the MessagePack tuple wire over those pipes. This is portable: it is
//! the same code on macOS / Windows / Linux (verified that `tsgo --api` answers
//! over stdio on Windows, so no named-pipe special-casing is needed).
//!
//! The transport implements [`DuplexTransport`](crate::actor::DuplexTransport):
//! `send_frame` writes complete frame bytes to the child's stdin; `recv_frame`
//! extracts the next complete tuple frame from the child's stdout via
//! [`read_one_frame`](crate::actor::read_one_frame).

use std::path::Path;
use std::process::Stdio;

use tokio::io::AsyncWriteExt;
use tokio::process::{Child, ChildStdin, ChildStdout};

use crate::actor::{read_one_frame, DuplexTransport};
use crate::error::{TsgoApiError, TsgoApiResult};
use crate::transport::spawn::build_sync_api_args;

/// A live tsgo `--api` process and its stdio pipes.
///
/// Holds the child handle so the process is killed when the transport is
/// dropped (via tokio's `kill_on_drop`). The actor owns this transport for the
/// session's lifetime.
#[derive(Debug)]
pub struct StdioPipeTransport {
    child: Child,
    stdin: ChildStdin,
    stdout: ChildStdout,
}

impl StdioPipeTransport {
    /// Spawn `tsgo --api` at `exe` with working directory `cwd`, enabling FS
    /// callbacks. `cwd` is the path the engine resolves project-relative paths
    /// against (it is also passed as `--cwd`).
    pub fn spawn(exe: &Path, cwd: &Path) -> TsgoApiResult<Self> {
        let cwd_str = cwd.to_string_lossy();
        let args = build_sync_api_args(&cwd_str, true);

        let mut child = tokio::process::Command::new(exe)
            .args(&args)
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| {
                TsgoApiError::Spawn(format!("failed to spawn tsgo at {}: {e}", exe.display()))
            })?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| TsgoApiError::Spawn("child stdin not piped".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| TsgoApiError::Spawn("child stdout not piped".into()))?;

        Ok(Self {
            child,
            stdin,
            stdout,
        })
    }

    /// Best-effort terminate the child process.
    pub async fn shutdown(&mut self) {
        // Closing stdin lets the engine's read loop see EOF and exit cleanly;
        // then ensure it is gone.
        let _ = self.stdin.shutdown().await;
        let _ = self.child.start_kill();
    }
}

impl DuplexTransport for StdioPipeTransport {
    async fn send_frame(&mut self, bytes: &[u8]) -> TsgoApiResult<()> {
        self.stdin
            .write_all(bytes)
            .await
            .map_err(|e| TsgoApiError::Transport(format!("write frame to stdin: {e}")))?;
        self.stdin
            .flush()
            .await
            .map_err(|e| TsgoApiError::Transport(format!("flush stdin: {e}")))
    }

    async fn recv_frame(&mut self) -> TsgoApiResult<Option<Vec<u8>>> {
        read_one_frame(&mut self.stdout).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A bogus exe path yields a typed Spawn error rather than a panic.
    #[tokio::test]
    async fn spawn_nonexistent_binary_is_typed_error() {
        let exe = Path::new("definitely-not-a-real-tsgo-binary-xyz");
        let cwd = std::env::temp_dir();
        let err = StdioPipeTransport::spawn(exe, &cwd).expect_err("must fail to spawn");
        assert!(matches!(err, TsgoApiError::Spawn(_)), "got {err:?}");
    }
}
