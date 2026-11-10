//! Packet framing and field readers/writers.
//!
//! Every modern Minecraft Java packet on the wire is:
//!
//! ```text
//! [VarInt length][VarInt packet id][payload]
//! ```
//!
//! where `length` counts the packet id and payload but not itself. In the login
//! and play states the payload may be compressed (zlib) once compression is
//! negotiated; that layer lives in the engine because it needs the negotiated
//! threshold, while this module provides the uncompressed framing and the field
//! primitives both layers share.

use std::fmt;

use crate::{read_varint, varint_bytes, write_varint, ProtocolError};

/// Hard ceiling on an uncompressed frame.
///
/// The vanilla client refuses packets larger than 2 MiB (`2097152`). ZWRS uses
/// the same limit for *input* so a hostile peer cannot force large allocations.
pub const MAX_PACKET_LEN: usize = 2 * 1024 * 1024;

/// Reads one length-prefixed frame from `buf`.
///
/// Returns the packet bytes (id + payload) and the total number of input bytes
/// consumed. When the buffer holds only part of a frame, returns
/// `Ok(None)` and the caller should read more from the socket.
///
/// # Errors
/// Fails when the declared length is zero, exceeds [`MAX_PACKET_LEN`], or when
/// the length prefix itself is malformed.
pub fn read_frame(buf: &[u8]) -> Result<Option<(&[u8], usize)>, ProtocolError> {
    let (len, prefix_len) = match read_varint(buf) {
        Ok(v) => v,
        Err(ProtocolError::Truncated { .. }) => return Ok(None),
        Err(e) => return Err(e),
    };
    if len < 0 {
        return Err(ProtocolError::BadLength {
            claimed: len.unsigned_abs() as usize,
            reason: "negative frame length",
        });
    }
    let len = len as usize;
    if len == 0 {
        return Err(ProtocolError::BadLength {
            claimed: 0,
            reason: "empty frame",
        });
    }
    if len > MAX_PACKET_LEN {
        return Err(ProtocolError::BadLength {
            claimed: len,
            reason: "frame exceeds 2 MiB limit",
        });
    }
    let total = prefix_len + len;
    if buf.len() < total {
        return Ok(None);
    }
    Ok(Some((&buf[prefix_len..total], total)))
}

/// Writes a length-prefixed frame around `packet`.
#[must_use]
pub fn write_frame(packet: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(packet.len() + 3);
    write_varint(&mut out, i32::try_from(packet.len()).unwrap_or(i32::MAX));
    out.extend_from_slice(packet);
    out
}

/// Cursor over a packet payload with checked field reads.
pub struct PacketReader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl fmt::Debug for PacketReader<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PacketReader")
            .field("len", &self.buf.len())
            .field("pos", &self.pos)
            .finish()
    }
}

impl<'a> PacketReader<'a> {
    /// Creates a reader over `buf`.
    #[must_use]
    pub const fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    /// Bytes not yet consumed.
    #[must_use]
    pub fn remaining(&self) -> usize {
        self.buf.len() - self.pos
    }

    /// Whether the payload was fully consumed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.remaining() == 0
    }

    fn need(&self, n: usize) -> Result<&'a [u8], ProtocolError> {
        if self.remaining() < n {
            return Err(ProtocolError::Truncated {
                needed: n,
                available: self.remaining(),
            });
        }
        let slice = &self.buf[self.pos..self.pos + n];
        Ok(slice)
    }

    /// VarInt field.
    ///
    /// # Errors
    /// See [`read_varint`].
    pub fn varint(&mut self) -> Result<i32, ProtocolError> {
        let (value, used) = read_varint(&self.buf[self.pos..])?;
        self.pos += used;
        Ok(value)
    }

    /// Unsigned byte field.
    ///
    /// # Errors
    /// [`ProtocolError::Truncated`] at end of payload.
    pub fn u8(&mut self) -> Result<u8, ProtocolError> {
        let b = self.need(1)?;
        self.pos += 1;
        Ok(b[0])
    }

    /// Signed byte field.
    ///
    /// # Errors
    /// [`ProtocolError::Truncated`] at end of payload.
    pub fn i8(&mut self) -> Result<i8, ProtocolError> {
        #[allow(clippy::cast_possible_wrap)]
        let b = self.need(1)?[0] as i8;
        self.pos += 1;
        Ok(b)
    }

    /// Big-endian `u16`.
    ///
    /// # Errors
    /// [`ProtocolError::Truncated`] at end of payload.
    pub fn u16(&mut self) -> Result<u16, ProtocolError> {
        let b = self.need(2)?;
        self.pos += 2;
        Ok(u16::from_be_bytes([b[0], b[1]]))
    }

    /// Big-endian `i32`.
    ///
    /// # Errors
    /// [`ProtocolError::Truncated`] at end of payload.
    pub fn i32(&mut self) -> Result<i32, ProtocolError> {
        let b = self.need(4)?;
        self.pos += 4;
        Ok(i32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }

    /// Big-endian `i64`.
    ///
    /// # Errors
    /// [`ProtocolError::Truncated`] at end of payload.
    pub fn i64(&mut self) -> Result<i64, ProtocolError> {
        let b = self.need(8)?;
        self.pos += 8;
        Ok(i64::from_be_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }

    /// Big-endian `f32`.
    ///
    /// # Errors
    /// [`ProtocolError::Truncated`] at end of payload.
    pub fn f32(&mut self) -> Result<f32, ProtocolError> {
        let b = self.need(4)?;
        self.pos += 4;
        Ok(f32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }

    /// Big-endian `f64`.
    ///
    /// # Errors
    /// [`ProtocolError::Truncated`] at end of payload.
    pub fn f64(&mut self) -> Result<f64, ProtocolError> {
        let b = self.need(8)?;
        self.pos += 8;
        Ok(f64::from_be_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }

    /// Boolean as a single byte.
    ///
    /// # Errors
    /// [`ProtocolError::Truncated`] at end of payload.
    pub fn bool(&mut self) -> Result<bool, ProtocolError> {
        Ok(self.u8()? != 0)
    }

    /// A UUID as two big-endian `i64`s.
    ///
    /// # Errors
    /// [`ProtocolError::Truncated`] at end of payload.
    pub fn uuid(&mut self) -> Result<[u8; 16], ProtocolError> {
        let b = self.need(16)?;
        let mut out = [0u8; 16];
        out.copy_from_slice(b);
        self.pos += 16;
        Ok(out)
    }

    /// Length-prefixed UTF-8 string, bounded by `max_chars`.
    ///
    /// # Errors
    /// Fails on truncation, over-long length prefixes, and invalid UTF-8.
    pub fn string(&mut self, max_chars: usize) -> Result<String, ProtocolError> {
        let len = usize::try_from(self.varint()?).map_err(|_| ProtocolError::StringTooLong {
            claimed: usize::MAX,
            max: max_chars,
        })?;
        // Each character is at least one byte on the wire, so a length prefix
        // greater than `max_chars` can never be valid.
        if len > max_chars * 4 {
            return Err(ProtocolError::StringTooLong {
                claimed: len,
                max: max_chars,
            });
        }
        let bytes = self.need(len)?;
        let s = std::str::from_utf8(bytes).map_err(|_| ProtocolError::InvalidUtf8)?;
        if s.chars().count() > max_chars {
            return Err(ProtocolError::StringTooLong {
                claimed: s.chars().count(),
                max: max_chars,
            });
        }
        self.pos += len;
        Ok(s.to_string())
    }

    /// Remaining bytes, consuming them.
    #[must_use]
    pub fn take_rest(&mut self) -> &'a [u8] {
        let slice = &self.buf[self.pos..];
        self.pos = self.buf.len();
        slice
    }
}

/// Builder for an outgoing packet payload.
#[derive(Debug, Default, Clone)]
pub struct PacketWriter {
    buf: Vec<u8>,
}

impl PacketWriter {
    /// Empty writer.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Writer pre-filled with a packet id.
    #[must_use]
    pub fn with_id(id: i32) -> Self {
        let mut w = Self::new();
        w.varint(id);
        w
    }

    /// Appends a VarInt.
    pub fn varint(&mut self, value: i32) -> &mut Self {
        write_varint(&mut self.buf, value);
        self
    }

    /// Appends a raw byte.
    pub fn u8(&mut self, value: u8) -> &mut Self {
        self.buf.push(value);
        self
    }

    /// Appends a big-endian `u16`.
    pub fn u16(&mut self, value: u16) -> &mut Self {
        self.buf.extend_from_slice(&value.to_be_bytes());
        self
    }

    /// Appends a big-endian `i32`.
    pub fn i32(&mut self, value: i32) -> &mut Self {
        self.buf.extend_from_slice(&value.to_be_bytes());
        self
    }

    /// Appends a big-endian `i64`.
    pub fn i64(&mut self, value: i64) -> &mut Self {
        self.buf.extend_from_slice(&value.to_be_bytes());
        self
    }

    /// Appends a boolean byte.
    pub fn bool(&mut self, value: bool) -> &mut Self {
        self.buf.push(u8::from(value));
        self
    }

    /// Appends a length-prefixed UTF-8 string.
    pub fn string(&mut self, value: &str) -> &mut Self {
        self.varint(i32::try_from(value.len()).unwrap_or(i32::MAX));
        self.buf.extend_from_slice(value.as_bytes());
        self
    }

    /// Appends a UUID field.
    pub fn uuid(&mut self, bytes: &[u8; 16]) -> &mut Self {
        self.buf.extend_from_slice(bytes);
        self
    }

    /// Appends raw bytes.
    pub fn bytes(&mut self, bytes: &[u8]) -> &mut Self {
        self.buf.extend_from_slice(bytes);
        self
    }

    /// Current length.
    #[must_use]
    pub fn len(&self) -> usize {
        self.buf.len()
    }

    /// Whether nothing has been written.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// Borrows the accumulated bytes.
    #[must_use]
    pub fn as_slice(&self) -> &[u8] {
        &self.buf
    }

    /// Finishes into a length-prefixed frame ready for the socket.
    #[must_use]
    pub fn into_frame(self) -> Vec<u8> {
        write_frame(&self.buf)
    }

    /// Finishes into the raw payload (no length prefix).
    #[must_use]
    pub fn into_payload(self) -> Vec<u8> {
        self.buf
    }
}

/// Encodes a `String` field for tests and hand-built packets.
#[must_use]
pub fn prefixed_string(value: &str) -> Vec<u8> {
    let mut out = varint_bytes(i32::try_from(value.len()).unwrap_or(i32::MAX));
    out.extend_from_slice(value.as_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_roundtrip() {
        let payload = vec![0x00, 0x01, 0x02];
        let framed = write_frame(&payload);
        let (got, used) = read_frame(&framed).unwrap().unwrap();
        assert_eq!(got, &payload[..]);
        assert_eq!(used, framed.len());
    }

    #[test]
    fn partial_frame_returns_none() {
        let mut framed = write_frame(&[1, 2, 3, 4, 5]);
        framed.truncate(3);
        assert!(read_frame(&framed).unwrap().is_none());
        assert!(read_frame(&[]).unwrap().is_none());
        assert!(read_frame(&[0x80]).unwrap().is_none());
    }

    #[test]
    fn rejects_zero_and_negative_lengths() {
        assert!(read_frame(&[0x00]).is_err());
        // -1 encoded as Five-byte VarInt.
        assert!(read_frame(&[0xff, 0xff, 0xff, 0xff, 0x0f]).is_err());
    }

    #[test]
    fn rejects_oversized_frame() {
        let mut huge = Vec::new();
        write_varint(&mut huge, (MAX_PACKET_LEN + 1) as i32);
        huge.extend_from_slice(&[0u8; 8]);
        assert!(matches!(
            read_frame(&huge),
            Err(ProtocolError::BadLength { .. })
        ));
    }

    #[test]
    fn field_roundtrip() {
        let mut w = PacketWriter::with_id(0x02);
        w.varint(-7)
            .u8(255)
            .u16(65535)
            .i32(-2_000_000)
            .i64(i64::MIN)
            .bool(true)
            .uuid(&[0xAB; 16])
            .string("héllo ✓");
        let payload = w.into_payload();
        let mut r = PacketReader::new(&payload);
        assert_eq!(r.varint().unwrap(), 0x02);
        assert_eq!(r.varint().unwrap(), -7);
        assert_eq!(r.u8().unwrap(), 255);
        assert_eq!(r.u16().unwrap(), 65535);
        assert_eq!(r.i32().unwrap(), -2_000_000);
        assert_eq!(r.i64().unwrap(), i64::MIN);
        assert!(r.bool().unwrap());
        assert_eq!(r.uuid().unwrap(), [0xAB; 16]);
        assert_eq!(r.string(32).unwrap(), "héllo ✓");
        assert!(r.is_empty());
    }

    #[test]
    fn string_bounds_are_enforced() {
        let mut w = PacketWriter::new();
        w.string(&"x".repeat(100));
        let payload = w.into_payload();
        let mut r = PacketReader::new(&payload);
        assert!(matches!(
            r.string(10),
            Err(ProtocolError::StringTooLong { .. })
        ));
    }

    #[test]
    fn truncated_fields_error() {
        let mut r = PacketReader::new(&[0x01]);
        assert!(r.i32().is_err());
        assert!(r.i64().is_err());
        assert!(r.uuid().is_err());
    }

    #[test]
    fn invalid_utf8_rejected() {
        let mut w = PacketWriter::new();
        w.varint(2).bytes(&[0xFF, 0xFE]);
        let payload = w.into_payload();
        assert_eq!(
            PacketReader::new(&payload).string(8),
            Err(ProtocolError::InvalidUtf8)
        );
    }

    #[test]
    fn floats_roundtrip() {
        let mut w = PacketWriter::new();
        w.bytes(&1.5f64.to_be_bytes()).bytes(&(-0.25f32).to_be_bytes());
        let payload = w.into_payload();
        let mut r = PacketReader::new(&payload);
        assert!((r.f64().unwrap() - 1.5).abs() < f64::EPSILON);
        assert!((r.f32().unwrap() + 0.25).abs() < f32::EPSILON);
    }
}

