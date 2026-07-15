//! Minimal MessagePack subset used by the tsgo `--api` wire.
//!
//! This is a faithful hand-written port of the shipped reference encoder/decoder
//! at `typescript/dist/api/node/msgpack.js`. The tsgo wire only
//! uses a tiny subset of MessagePack — arrays, unsigned integers, strings,
//! booleans, and binary blobs — all big-endian. We mirror exactly those, byte
//! for byte, so the bytes this writer emits are identical to the JS writer's
//! and the reader accepts exactly what the JS reader accepts.
//!
//! Mirrored format constants (msgpack.js:4-8):
//! - `MSGPACK_FIXARRAY3 = 0x93` — a 3-element fixarray header.
//! - `MSGPACK_BIN8 = 0xc4`, `MSGPACK_BIN16 = 0xc5`, `MSGPACK_BIN32 = 0xc6`.
//! - `MSGPACK_UINT8 = 0xcc`.
//!
//! Marker families mirrored from `MsgpackWriter`/`MsgpackReader` (msgpack.js:40-211):
//! - array header: `0x90|len` (len ≤ 15), `0xdc` + u16 (≤ 0xffff), `0xdd` + u32.
//! - uint: `value` (≤ 0x7f), `0xcc` + u8, `0xcd` + u16, `0xce` + u32.
//! - string: `0xa0|len` (≤ 0x1f), `0xd9` + u8, `0xda` + u16, `0xdb` + u32.
//! - bool: `0xc3` (true), `0xc2` (false).
//! - bin: `0xc4` + u8, `0xc5` + u16, `0xc6` + u32 (header via [`write_bin_header`]).

use crate::error::{TsgoApiError, TsgoApiResult};

// ── MessagePack format constants (msgpack.js:4-8) ───────────────────────────
/// 3-element fixarray header byte (`0x93`). Mirrors `MSGPACK_FIXARRAY3`.
pub const MSGPACK_FIXARRAY3: u8 = 0x93;
/// `bin 8` marker. Mirrors `MSGPACK_BIN8`.
pub const MSGPACK_BIN8: u8 = 0xc4;
/// `bin 16` marker. Mirrors `MSGPACK_BIN16`.
pub const MSGPACK_BIN16: u8 = 0xc5;
/// `bin 32` marker. Mirrors `MSGPACK_BIN32`.
pub const MSGPACK_BIN32: u8 = 0xc6;
/// `uint 8` marker. Mirrors `MSGPACK_UINT8`.
pub const MSGPACK_UINT8: u8 = 0xcc;

/// Compute the MessagePack `bin` header size for a given data length.
///
/// Mirrors `binHeaderSize` (msgpack.js:11-17): BIN8 → 2, BIN16 → 3, BIN32 → 5.
pub fn bin_header_size(len: usize) -> usize {
    if len < 0x100 {
        2
    } else if len < 0x10000 {
        3
    } else {
        5
    }
}

/// Append a MessagePack `bin` header for `len` payload bytes to `out`.
///
/// Mirrors `writeBinHeader` (msgpack.js:19-37). Big-endian sizes.
pub fn write_bin_header(out: &mut Vec<u8>, len: usize) {
    if len < 0x100 {
        out.push(MSGPACK_BIN8);
        out.push(len as u8);
    } else if len < 0x10000 {
        out.push(MSGPACK_BIN16);
        out.extend_from_slice(&(len as u16).to_be_bytes());
    } else {
        out.push(MSGPACK_BIN32);
        out.extend_from_slice(&(len as u32).to_be_bytes());
    }
}

/// A growable MessagePack writer producing exactly the bytes the JS
/// `MsgpackWriter` produces. Mirrors `MsgpackWriter` (msgpack.js:40-135).
#[derive(Debug, Default)]
pub struct MsgpackWriter {
    buf: Vec<u8>,
}

impl MsgpackWriter {
    /// Create an empty writer.
    pub fn new() -> Self {
        Self { buf: Vec::new() }
    }

    /// Write an array header for `length` elements. Mirrors `writeArrayHeader`
    /// (msgpack.js:60-77): `0x90|len`, `0xdc`+u16, or `0xdd`+u32 (big-endian).
    pub fn write_array_header(&mut self, length: usize) {
        if length <= 0x0f {
            self.buf.push(0x90 | (length as u8));
        } else if length <= 0xffff {
            self.buf.push(0xdc);
            self.buf.extend_from_slice(&(length as u16).to_be_bytes());
        } else {
            self.buf.push(0xdd);
            self.buf.extend_from_slice(&(length as u32).to_be_bytes());
        }
    }

    /// Write an unsigned integer. Mirrors `writeUint` (msgpack.js:78-100):
    /// fixint, `0xcc`+u8, `0xcd`+u16, or `0xce`+u32 (big-endian).
    pub fn write_uint(&mut self, value: u32) {
        if value <= 0x7f {
            self.buf.push(value as u8);
        } else if value <= 0xff {
            self.buf.push(0xcc);
            self.buf.push(value as u8);
        } else if value <= 0xffff {
            self.buf.push(0xcd);
            self.buf.extend_from_slice(&(value as u16).to_be_bytes());
        } else {
            self.buf.push(0xce);
            self.buf.extend_from_slice(&value.to_be_bytes());
        }
    }

    /// Write a UTF-8 string. Mirrors `writeString` (msgpack.js:101-127):
    /// `0xa0|len`, `0xd9`+u8, `0xda`+u16, or `0xdb`+u32 (big-endian) then bytes.
    /// The length is the UTF-8 byte length, not the char count.
    pub fn write_string(&mut self, s: &str) {
        let bytes = s.as_bytes();
        let len = bytes.len();
        if len <= 0x1f {
            self.buf.push(0xa0 | (len as u8));
        } else if len <= 0xff {
            self.buf.push(0xd9);
            self.buf.push(len as u8);
        } else if len <= 0xffff {
            self.buf.push(0xda);
            self.buf.extend_from_slice(&(len as u16).to_be_bytes());
        } else {
            self.buf.push(0xdb);
            self.buf.extend_from_slice(&(len as u32).to_be_bytes());
        }
        self.buf.extend_from_slice(bytes);
    }

    /// Write a boolean. Mirrors `writeBool` (msgpack.js:128-131): `0xc3`/`0xc2`.
    pub fn write_bool(&mut self, value: bool) {
        self.buf.push(if value { 0xc3 } else { 0xc2 });
    }

    /// Write a `bin` field: a [`write_bin_header`] followed by the raw bytes.
    /// The JS side emits this shape for the tuple `name`/`payload` fields
    /// (syncChannel.js:264-317).
    pub fn write_bin(&mut self, data: &[u8]) {
        write_bin_header(&mut self.buf, data.len());
        self.buf.extend_from_slice(data);
    }

    /// Consume the writer and return the produced bytes. Mirrors `finish`
    /// (msgpack.js:132-134).
    pub fn finish(self) -> Vec<u8> {
        self.buf
    }

    /// Borrow the produced bytes so far (test/inspection helper).
    pub fn as_slice(&self) -> &[u8] {
        &self.buf
    }
}

/// A MessagePack reader over a borrowed byte slice. Mirrors `MsgpackReader`
/// (msgpack.js:136-211). Every reader method advances the cursor and returns a
/// typed [`TsgoApiError::Codec`] on a marker/length mismatch.
#[derive(Debug)]
pub struct MsgpackReader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> MsgpackReader<'a> {
    /// Create a reader positioned at `offset` within `data`.
    pub fn new(data: &'a [u8], offset: usize) -> Self {
        Self {
            buf: data,
            pos: offset,
        }
    }

    /// Current cursor position (bytes consumed from the start of `data`).
    pub fn position(&self) -> usize {
        self.pos
    }

    /// True when the cursor has consumed the entire slice.
    pub fn is_at_end(&self) -> bool {
        self.pos >= self.buf.len()
    }

    /// Read one raw byte, advancing the cursor. Errors on EOF.
    fn read_byte(&mut self) -> TsgoApiResult<u8> {
        let b = *self
            .buf
            .get(self.pos)
            .ok_or_else(|| TsgoApiError::Codec("unexpected end of input".to_string()))?;
        self.pos += 1;
        Ok(b)
    }

    /// Read `n` raw bytes as a big-endian length helper. Errors on truncation.
    fn read_len_be(&mut self, n: usize) -> TsgoApiResult<usize> {
        if self.pos + n > self.buf.len() {
            return Err(TsgoApiError::Codec(format!(
                "truncated {n}-byte length field"
            )));
        }
        let mut acc: usize = 0;
        for _ in 0..n {
            acc = (acc << 8) | (self.buf[self.pos] as usize);
            self.pos += 1;
        }
        Ok(acc)
    }

    /// Take `len` bytes from the cursor, returning a borrowed slice. Errors on
    /// truncation.
    fn take(&mut self, len: usize) -> TsgoApiResult<&'a [u8]> {
        let end = self
            .pos
            .checked_add(len)
            .ok_or_else(|| TsgoApiError::Codec("length overflow".to_string()))?;
        if end > self.buf.len() {
            return Err(TsgoApiError::Codec(format!(
                "truncated payload: need {len} bytes, have {}",
                self.buf.len() - self.pos
            )));
        }
        let slice = &self.buf[self.pos..end];
        self.pos = end;
        Ok(slice)
    }

    /// Read an array header, returning the element count. Mirrors
    /// `readArrayHeader` (msgpack.js:145-160).
    pub fn read_array_header(&mut self) -> TsgoApiResult<usize> {
        let byte = self.read_byte()?;
        if byte & 0xf0 == 0x90 {
            return Ok((byte & 0x0f) as usize);
        }
        match byte {
            0xdc => self.read_len_be(2),
            0xdd => self.read_len_be(4),
            other => Err(TsgoApiError::Codec(format!(
                "expected array header, got {other:#04x}"
            ))),
        }
    }

    /// Read an unsigned integer. Mirrors `readUint` (msgpack.js:161-178).
    pub fn read_uint(&mut self) -> TsgoApiResult<u32> {
        let byte = self.read_byte()?;
        if byte <= 0x7f {
            return Ok(byte as u32);
        }
        match byte {
            MSGPACK_UINT8 => Ok(self.read_byte()? as u32),
            0xcd => Ok(self.read_len_be(2)? as u32),
            0xce => Ok(self.read_len_be(4)? as u32),
            other => Err(TsgoApiError::Codec(format!(
                "expected uint, got {other:#04x}"
            ))),
        }
    }

    /// Read a UTF-8 string. Mirrors `readString` (msgpack.js:179-202).
    pub fn read_string(&mut self) -> TsgoApiResult<String> {
        let byte = self.read_byte()?;
        let len = if byte & 0xe0 == 0xa0 {
            (byte & 0x1f) as usize
        } else {
            match byte {
                0xd9 => self.read_byte()? as usize,
                0xda => self.read_len_be(2)?,
                0xdb => self.read_len_be(4)?,
                other => {
                    return Err(TsgoApiError::Codec(format!(
                        "expected string, got {other:#04x}"
                    )))
                }
            }
        };
        let bytes = self.take(len)?;
        std::str::from_utf8(bytes)
            .map(|s| s.to_string())
            .map_err(|e| TsgoApiError::Codec(format!("invalid utf-8 in string: {e}")))
    }

    /// Read a boolean. Mirrors `readBool` (msgpack.js:203-210).
    pub fn read_bool(&mut self) -> TsgoApiResult<bool> {
        match self.read_byte()? {
            0xc3 => Ok(true),
            0xc2 => Ok(false),
            other => Err(TsgoApiError::Codec(format!(
                "expected bool, got {other:#04x}"
            ))),
        }
    }

    /// Read a `bin` field, returning the borrowed payload bytes. Mirrors the
    /// `readBin` logic in the channel reader (syncChannel.js:347-368).
    pub fn read_bin(&mut self) -> TsgoApiResult<&'a [u8]> {
        let marker = self.read_byte()?;
        let len = match marker {
            MSGPACK_BIN8 => self.read_byte()? as usize,
            MSGPACK_BIN16 => self.read_len_be(2)?,
            MSGPACK_BIN32 => self.read_len_be(4)?,
            other => {
                return Err(TsgoApiError::Codec(format!(
                    "expected binary data (0xc4-0xc6), got {other:#04x}"
                )))
            }
        };
        self.take(len)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── bin_header_size mirrors msgpack.js:11-17 ────────────────────────────
    #[test]
    fn bin_header_size_matches_reference_thresholds() {
        assert_eq!(bin_header_size(0), 2);
        assert_eq!(bin_header_size(0xff), 2);
        assert_eq!(bin_header_size(0x100), 3);
        assert_eq!(bin_header_size(0xffff), 3);
        assert_eq!(bin_header_size(0x10000), 5);
    }

    // ── write_bin_header mirrors msgpack.js:19-37 (big-endian) ──────────────
    #[test]
    fn write_bin_header_emits_exact_reference_bytes() {
        let mut b = Vec::new();
        write_bin_header(&mut b, 5);
        assert_eq!(b, vec![MSGPACK_BIN8, 5], "BIN8 marker + 1-byte len");

        let mut b = Vec::new();
        write_bin_header(&mut b, 0x1234);
        assert_eq!(
            b,
            vec![MSGPACK_BIN16, 0x12, 0x34],
            "BIN16 marker + big-endian u16"
        );

        let mut b = Vec::new();
        write_bin_header(&mut b, 0x0001_0002);
        assert_eq!(
            b,
            vec![MSGPACK_BIN32, 0x00, 0x01, 0x00, 0x02],
            "BIN32 marker + big-endian u32"
        );
    }

    // ── array header: 0x90|len, 0xdc+u16, 0xdd+u32 (msgpack.js:60-77) ────────
    #[test]
    fn array_header_fixarray_and_extended_forms() {
        let mut w = MsgpackWriter::new();
        w.write_array_header(3);
        assert_eq!(w.as_slice(), &[0x93], "fixarray3 is exactly 0x93");

        let mut w = MsgpackWriter::new();
        w.write_array_header(0x0f);
        assert_eq!(w.as_slice(), &[0x9f]);

        let mut w = MsgpackWriter::new();
        w.write_array_header(0x10);
        assert_eq!(w.as_slice(), &[0xdc, 0x00, 0x10], "array16 big-endian");

        let mut w = MsgpackWriter::new();
        w.write_array_header(0x0001_0000);
        assert_eq!(
            w.as_slice(),
            &[0xdd, 0x00, 0x01, 0x00, 0x00],
            "array32 big-endian"
        );
    }

    // ── uint: fixint, 0xcc, 0xcd, 0xce (msgpack.js:78-100) ───────────────────
    #[test]
    fn uint_encoding_picks_smallest_form_big_endian() {
        let cases: &[(u32, &[u8])] = &[
            (0x00, &[0x00]),
            (0x7f, &[0x7f]),
            (0x80, &[0xcc, 0x80]),
            (0xff, &[0xcc, 0xff]),
            (0x0100, &[0xcd, 0x01, 0x00]),
            (0xffff, &[0xcd, 0xff, 0xff]),
            (0x0001_0000, &[0xce, 0x00, 0x01, 0x00, 0x00]),
            (0xffff_ffff, &[0xce, 0xff, 0xff, 0xff, 0xff]),
        ];
        for (value, expected) in cases {
            let mut w = MsgpackWriter::new();
            w.write_uint(*value);
            assert_eq!(w.as_slice(), *expected, "uint {value:#x}");
        }
    }

    // ── string: fixstr, str8, str16, str32 (msgpack.js:101-127) ──────────────
    #[test]
    fn string_encoding_forms_and_payload() {
        let mut w = MsgpackWriter::new();
        w.write_string("ab");
        assert_eq!(w.as_slice(), &[0xa2, b'a', b'b'], "fixstr 0xa0|len + bytes");

        // 0x20-byte string crosses into str8.
        let s = "x".repeat(0x20);
        let mut w = MsgpackWriter::new();
        w.write_string(&s);
        let mut expected = vec![0xd9, 0x20];
        expected.extend(std::iter::repeat_n(b'x', 0x20));
        assert_eq!(
            w.as_slice(),
            expected.as_slice(),
            "str8 marker + len + bytes"
        );

        // UTF-8 length is byte length, not char count.
        let mut w = MsgpackWriter::new();
        w.write_string("é"); // 2 UTF-8 bytes
        assert_eq!(w.as_slice(), &[0xa2, 0xc3, 0xa9]);
    }

    #[test]
    fn bool_encoding_matches_reference() {
        let mut w = MsgpackWriter::new();
        w.write_bool(true);
        w.write_bool(false);
        assert_eq!(w.as_slice(), &[0xc3, 0xc2]);
    }

    #[test]
    fn bin_field_header_plus_bytes() {
        let mut w = MsgpackWriter::new();
        w.write_bin(b"hi");
        assert_eq!(w.as_slice(), &[MSGPACK_BIN8, 2, b'h', b'i']);

        let mut w = MsgpackWriter::new();
        w.write_bin(&[]);
        assert_eq!(w.as_slice(), &[MSGPACK_BIN8, 0], "empty bin is BIN8 + 0");
    }

    // ── round-trips: writer output must decode back identically ─────────────
    #[test]
    fn roundtrip_array_uint_string_bool_bin() {
        let mut w = MsgpackWriter::new();
        w.write_array_header(4);
        w.write_uint(0x1234);
        w.write_string("hello");
        w.write_bool(true);
        w.write_bin(b"\x00\x01\x02");
        let bytes = w.finish();

        let mut r = MsgpackReader::new(&bytes, 0);
        assert_eq!(r.read_array_header().unwrap(), 4);
        assert_eq!(r.read_uint().unwrap(), 0x1234);
        assert_eq!(r.read_string().unwrap(), "hello");
        assert!(r.read_bool().unwrap());
        assert_eq!(r.read_bin().unwrap(), b"\x00\x01\x02");
        assert!(r.is_at_end(), "reader consumed every byte");
    }

    // ── DISCRIMINATING: wrong marker family must be a typed Codec error ──────
    #[test]
    fn reader_rejects_wrong_markers() {
        // 0x93 is a fixarray, not a uint.
        let mut r = MsgpackReader::new(&[0x93], 0);
        assert!(
            matches!(r.read_uint(), Err(TsgoApiError::Codec(_))),
            "reading a uint where an array header sits must error"
        );

        // 0xc3 (bool true) is not a string.
        let mut r = MsgpackReader::new(&[0xc3], 0);
        assert!(matches!(r.read_string(), Err(TsgoApiError::Codec(_))));

        // 0x00 (fixint) is not a bool.
        let mut r = MsgpackReader::new(&[0x00], 0);
        assert!(matches!(r.read_bool(), Err(TsgoApiError::Codec(_))));

        // 0xa0 (fixstr) is not a bin.
        let mut r = MsgpackReader::new(&[0xa0], 0);
        assert!(matches!(r.read_bin(), Err(TsgoApiError::Codec(_))));
    }

    #[test]
    fn reader_rejects_truncated_input() {
        // BIN8 claims 4 bytes but only 1 follows the header.
        let mut r = MsgpackReader::new(&[MSGPACK_BIN8, 0x04, 0xaa], 0);
        assert!(matches!(r.read_bin(), Err(TsgoApiError::Codec(_))));

        // uint16 marker with only 1 trailing byte.
        let mut r = MsgpackReader::new(&[0xcd, 0x01], 0);
        assert!(matches!(r.read_uint(), Err(TsgoApiError::Codec(_))));
    }

    // ── NEGATIVE: a fixarray of length != 3 is still a valid array header ────
    //    (the *frame* layer enforces 3; the msgpack layer must read any len).
    #[test]
    fn array_header_reads_non_three_lengths() {
        let mut r = MsgpackReader::new(&[0x91], 0);
        assert_eq!(r.read_array_header().unwrap(), 1);
        let mut r = MsgpackReader::new(&[0x95], 0);
        assert_eq!(r.read_array_header().unwrap(), 5);
    }
}
