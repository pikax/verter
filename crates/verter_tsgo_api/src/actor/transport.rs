//! The duplex transport abstraction the actor drives.
//!
//! The actor needs to (a) write a complete tuple frame and (b) read the next
//! complete tuple frame. The framing is the same on every backend, so the trait
//! is expressed at the FRAME level: backends shuttle whole frames and never see
//! partial bytes. The real OS backend (named pipe on Windows, stdio on Unix)
//! extracts complete frames from its byte stream with [`read_one_frame`]; the
//! in-memory test backend ([`FrameStream`]) delivers pre-built frames directly.

use crate::error::{TsgoApiError, TsgoApiResult};
use crate::proto::msgpack::{
    MSGPACK_BIN16, MSGPACK_BIN32, MSGPACK_BIN8, MSGPACK_FIXARRAY3, MSGPACK_UINT8,
};

/// A duplex, frame-oriented transport: send a complete frame, receive the next
/// complete frame. Implementations must be cancel-safe at the frame boundary
/// (a `recv_frame` future may be dropped between frames).
///
/// The methods return `impl Future + Send` (rather than a bare `async fn`) so
/// the actor task that drives a transport is `Send` and can run on tokio's
/// multi-thread runtime.
pub trait DuplexTransport: Send {
    /// Write a complete tuple frame to the engine.
    fn send_frame(
        &mut self,
        bytes: &[u8],
    ) -> impl std::future::Future<Output = TsgoApiResult<()>> + Send;

    /// Read the next complete tuple frame from the engine. `Ok(None)` signals a
    /// clean EOF (the engine closed the connection).
    fn recv_frame(
        &mut self,
    ) -> impl std::future::Future<Output = TsgoApiResult<Option<Vec<u8>>>> + Send;

    /// Terminate the underlying engine IMMEDIATELY — a process-tree kill where
    /// the transport owns a process. The actor calls this when a request
    /// deadline fires: the single-flight wire cannot recover a wedged request,
    /// so the engine is torn down rather than left to hang the next request.
    /// Default: no-op (in-memory transports own no process).
    fn terminate(&mut self) -> impl std::future::Future<Output = ()> + Send {
        async {}
    }
}

/// Read exactly one complete tuple frame from an [`AsyncRead`], returning its
/// bytes (including the leading `0x93`). Returns `Ok(None)` on a clean EOF at a
/// frame boundary. This is the shared framing logic for any byte-stream backend
/// (the OS pipe), mirroring `readTuple` (syncChannel.js:324-368): the frame is
/// `[fixarray3, type, name-bin, payload-bin]`, and the two bin fields' lengths
/// tell us exactly how many bytes the frame occupies.
pub async fn read_one_frame<R>(reader: &mut R) -> TsgoApiResult<Option<Vec<u8>>>
where
    R: tokio::io::AsyncRead + Unpin,
{
    use tokio::io::AsyncReadExt;

    // 1. Leading array marker (or clean EOF).
    let mut marker = [0u8; 1];
    match reader.read(&mut marker).await {
        Ok(0) => return Ok(None), // clean EOF at a frame boundary
        Ok(_) => {}
        Err(e) => return Err(TsgoApiError::Transport(format!("read marker: {e}"))),
    }
    if marker[0] != MSGPACK_FIXARRAY3 {
        return Err(TsgoApiError::Codec(format!(
            "expected fixarray3 (0x93) frame marker, got {:#04x}",
            marker[0]
        )));
    }
    let mut frame = vec![MSGPACK_FIXARRAY3];

    // 2. Message-type byte (positive fixint or 0xcc + u8).
    let type_marker = read_u8(reader).await?;
    frame.push(type_marker);
    if type_marker == MSGPACK_UINT8 {
        frame.push(read_u8(reader).await?);
    } else if type_marker > 0x7f {
        return Err(TsgoApiError::Codec(format!(
            "invalid message-type marker {type_marker:#04x}"
        )));
    }

    // 3. Two bin fields (name, payload): read each header, then its bytes.
    for _ in 0..2 {
        read_bin_field_into(reader, &mut frame).await?;
    }

    Ok(Some(frame))
}

async fn read_u8<R>(reader: &mut R) -> TsgoApiResult<u8>
where
    R: tokio::io::AsyncRead + Unpin,
{
    use tokio::io::AsyncReadExt;
    reader
        .read_u8()
        .await
        .map_err(|e| TsgoApiError::Transport(format!("read byte: {e}")))
}

/// Read one `bin` field (header + bytes) from `reader`, appending the raw bytes
/// (header included) to `frame`.
async fn read_bin_field_into<R>(reader: &mut R, frame: &mut Vec<u8>) -> TsgoApiResult<()>
where
    R: tokio::io::AsyncRead + Unpin,
{
    use tokio::io::AsyncReadExt;

    let bin_marker = read_u8(reader).await?;
    frame.push(bin_marker);
    let len = match bin_marker {
        MSGPACK_BIN8 => {
            let n = read_u8(reader).await?;
            frame.push(n);
            n as usize
        }
        MSGPACK_BIN16 => {
            let mut b = [0u8; 2];
            reader
                .read_exact(&mut b)
                .await
                .map_err(|e| TsgoApiError::Transport(format!("read bin16 len: {e}")))?;
            frame.extend_from_slice(&b);
            u16::from_be_bytes(b) as usize
        }
        MSGPACK_BIN32 => {
            let mut b = [0u8; 4];
            reader
                .read_exact(&mut b)
                .await
                .map_err(|e| TsgoApiError::Transport(format!("read bin32 len: {e}")))?;
            frame.extend_from_slice(&b);
            u32::from_be_bytes(b) as usize
        }
        other => {
            return Err(TsgoApiError::Codec(format!(
                "expected bin marker (0xc4-0xc6), got {other:#04x}"
            )))
        }
    };

    let start = frame.len();
    frame.resize(start + len, 0);
    reader
        .read_exact(&mut frame[start..])
        .await
        .map_err(|e| TsgoApiError::Transport(format!("read bin payload ({len} bytes): {e}")))?;
    Ok(())
}

/// An in-memory duplex transport for tests and for any backend that already has
/// complete frames in hand. Frames written via `send_frame` are pushed to
/// `sent`; frames to be received are pulled from an mpsc channel.
///
/// This is also the shape the real OS transport adapts to: a reader task does
/// [`read_one_frame`] in a loop and forwards complete frames into the inbound
/// channel; a writer half owns the pipe write end.
#[derive(Debug)]
pub struct FrameStream {
    inbound: tokio::sync::mpsc::Receiver<Vec<u8>>,
    outbound: tokio::sync::mpsc::Sender<Vec<u8>>,
}

impl FrameStream {
    /// Create a `FrameStream` from an inbound (engine → actor) receiver and an
    /// outbound (actor → engine) sender.
    pub fn new(
        inbound: tokio::sync::mpsc::Receiver<Vec<u8>>,
        outbound: tokio::sync::mpsc::Sender<Vec<u8>>,
    ) -> Self {
        Self { inbound, outbound }
    }
}

impl DuplexTransport for FrameStream {
    async fn send_frame(&mut self, bytes: &[u8]) -> TsgoApiResult<()> {
        self.outbound
            .send(bytes.to_vec())
            .await
            .map_err(|_| TsgoApiError::Transport("frame sink closed".into()))
    }

    async fn recv_frame(&mut self) -> TsgoApiResult<Option<Vec<u8>>> {
        Ok(self.inbound.recv().await)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::frame::{encode_frame, MessageType};

    #[tokio::test]
    async fn read_one_frame_extracts_a_single_frame() {
        let frame = encode_frame(MessageType::Response, b"initialize", br#"{"ok":true}"#);
        let mut cursor = std::io::Cursor::new(frame.clone());
        let got = read_one_frame(&mut cursor).await.unwrap().unwrap();
        assert_eq!(got, frame);
    }

    #[tokio::test]
    async fn read_one_frame_extracts_frames_back_to_back() {
        let f1 = encode_frame(MessageType::Response, b"a", b"1");
        let f2 = encode_frame(MessageType::Call, b"readFile", br#""/x.ts""#);
        let mut buf = f1.clone();
        buf.extend_from_slice(&f2);
        let mut cursor = std::io::Cursor::new(buf);
        assert_eq!(read_one_frame(&mut cursor).await.unwrap().unwrap(), f1);
        assert_eq!(read_one_frame(&mut cursor).await.unwrap().unwrap(), f2);
        // A third read at EOF is a clean None.
        assert_eq!(read_one_frame(&mut cursor).await.unwrap(), None);
    }

    #[tokio::test]
    async fn read_one_frame_handles_bin16_payload() {
        // A payload over 255 bytes uses BIN16; the frame reader must follow it.
        let payload = vec![b'x'; 300];
        let frame = encode_frame(MessageType::Response, b"m", &payload);
        let mut cursor = std::io::Cursor::new(frame.clone());
        let got = read_one_frame(&mut cursor).await.unwrap().unwrap();
        assert_eq!(got, frame);
        // And it decodes back to the same payload.
        let (decoded, _) = crate::proto::frame::decode_frame(&got, 0).unwrap();
        assert_eq!(decoded.payload.len(), 300);
    }

    #[tokio::test]
    async fn read_one_frame_rejects_non_frame_marker() {
        let mut cursor = std::io::Cursor::new(vec![0x00, 0x01, 0x02]);
        assert!(matches!(
            read_one_frame(&mut cursor).await,
            Err(TsgoApiError::Codec(_))
        ));
    }
}
