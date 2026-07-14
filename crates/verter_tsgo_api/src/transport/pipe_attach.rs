//! Portable client transport for the `--api` ATTACH pipe.
//!
//! After the `tsgo --lsp` server answers `custom/initializeAPISession` with a
//! `pipe` path, this connects to it: a Windows named pipe (`\\.\pipe\…`) via
//! `tokio::net::windows::named_pipe`, or a Unix-domain socket via
//! `tokio::net::UnixStream` on macOS / Linux. The server mints the full path; the
//! caller passes it through VERBATIM (mirroring the shipped async client's
//! `net.createConnection(options.pipe)`), so there is NO hardcoded path here.
//!
//! The connected stream is split into boxed [`AsyncRead`] / [`AsyncWrite`] halves
//! so the per-OS concrete socket types unify behind one return type that
//! [`crate::jsonrpc::JsonRpcConnection`] can drive.

use tokio::io::{AsyncRead, AsyncWrite};

use crate::error::{TsgoApiError, TsgoApiResult};

/// A boxed read half of an attach connection.
pub type AttachReadHalf = Box<dyn AsyncRead + Unpin + Send + 'static>;
/// A boxed write half of an attach connection.
pub type AttachWriteHalf = Box<dyn AsyncWrite + Unpin + Send + 'static>;

/// Connect to the server-minted `--api` pipe and return its split read/write
/// halves. `pipe` is the exact path the `custom/initializeAPISession` response
/// carried (a `\\.\pipe\tsgo-api-…` name on Windows, a UDS path on Unix).
///
/// On Windows a named-pipe client connect can transiently fail with
/// `ERROR_PIPE_BUSY` while the server is between `ConnectNamedPipe` calls; this
/// retries briefly before giving up, matching the platform's documented client
/// connect protocol.
pub async fn connect_attach_pipe(pipe: &str) -> TsgoApiResult<(AttachReadHalf, AttachWriteHalf)> {
    #[cfg(windows)]
    {
        connect_windows_named_pipe(pipe).await
    }
    #[cfg(unix)]
    {
        connect_unix_socket(pipe).await
    }
    #[cfg(not(any(windows, unix)))]
    {
        let _ = pipe;
        Err(TsgoApiError::Transport(
            "the --api attach pipe transport is only implemented for Windows and Unix".to_string(),
        ))
    }
}

#[cfg(windows)]
async fn connect_windows_named_pipe(
    pipe: &str,
) -> TsgoApiResult<(AttachReadHalf, AttachWriteHalf)> {
    use std::time::Duration;
    use tokio::net::windows::named_pipe::ClientOptions;

    // ERROR_PIPE_BUSY: all pipe instances are busy. Retry a bounded number of
    // times with a short backoff (the documented Windows client-connect dance).
    const ERROR_PIPE_BUSY: i32 = 231;
    const MAX_ATTEMPTS: u32 = 50;

    let mut attempt = 0u32;
    let client = loop {
        match ClientOptions::new().open(pipe) {
            Ok(c) => break c,
            Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY) && attempt < MAX_ATTEMPTS => {
                attempt += 1;
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            Err(e) => {
                return Err(TsgoApiError::Transport(format!(
                    "connect to --api named pipe {pipe:?} failed: {e}"
                )))
            }
        }
    };
    let (read, write) = tokio::io::split(client);
    Ok((Box::new(read), Box::new(write)))
}

#[cfg(unix)]
async fn connect_unix_socket(pipe: &str) -> TsgoApiResult<(AttachReadHalf, AttachWriteHalf)> {
    use tokio::net::UnixStream;

    let stream = UnixStream::connect(pipe).await.map_err(|e| {
        TsgoApiError::Transport(format!("connect to --api unix socket {pipe:?} failed: {e}"))
    })?;
    let (read, write) = tokio::io::split(stream);
    Ok((Box::new(read), Box::new(write)))
}

#[cfg(test)]
#[path = "pipe_attach_tests.rs"]
mod tests;
