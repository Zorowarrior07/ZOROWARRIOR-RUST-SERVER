//! Minecraft VarInt / VarLong encoding.
//!
//! The Minecraft protocol uses a LEB128-style variable-length integer with a
//! **maximum of 5 bytes** (VarInt, 32-bit) or **10 bytes** (VarLong, 64-bit).
//! Over-long encodings are rejected: a peer that sends six bytes for an int is
//! malformed and must not be able to make the server loop.

use crate::ProtocolError;

/// Maximum wire length of a VarInt.
pub const MAX_VARINT_BYTES: usize = 5;
/// Maximum wire length of a VarLong.
pub const MAX_VARLONG_BYTES: usize = 10;

/// Encodes an `i32` as a VarInt, appending to `out`.
pub fn write_varint(out: &mut Vec<u8>, value: i32) {
    let mut v = u32::from_le_bytes(value.to_le_bytes());
    loop {
        #[allow(clippy::cast_possible_truncation)]
        let byte = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(byte);
            break;
        }
        out.push(byte | 0x80);
    }
}

/// Encodes an `i64` as a VarLong, appending to `out`.
pub fn write_varlong(out: &mut Vec<u8>, value: i64) {
    let mut v = u64::from_le_bytes(value.to_le_bytes());
    loop {
        #[allow(clippy::cast_possible_truncation)]
        let byte = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(byte);
            break;
        }
        out.push(byte | 0x80);
    }
}

/// Decodes a VarInt, returning the value and the number of bytes consumed.
///
/// # Errors
/// [`ProtocolError::Truncated`] when the buffer ends mid-value and
/// [`ProtocolError::VarIntTooLong`] when more than five bytes are used.
pub fn read_varint(buf: &[u8]) -> Result<(i32, usize), ProtocolError> {
    let mut value: u32 = 0;
    for (i, &byte) in buf.iter().enumerate().take(MAX_VARINT_BYTES) {
        value |= u32::from(byte & 0x7f) << (7 * i);
        if byte & 0x80 == 0 {
            return Ok((i32::from_le_bytes(value.to_le_bytes()), i + 1));
        }
    }
    if buf.len() < MAX_VARINT_BYTES {
        Err(ProtocolError::Truncated {
            needed: MAX_VARINT_BYTES - buf.len(),
            available: 0,
        })
    } else {
        Err(ProtocolError::VarIntTooLong {
            max: MAX_VARINT_BYTES,
        })
    }
}

/// Decodes a VarLong, returning the value and the number of bytes consumed.
///
/// # Errors
/// See [`read_varint`].
pub fn read_varlong(buf: &[u8]) -> Result<(i64, usize), ProtocolError> {
    let mut value: u64 = 0;
    for (i, &byte) in buf.iter().enumerate().take(MAX_VARLONG_BYTES) {
        value |= u64::from(byte & 0x7f) << (7 * i);
        if byte & 0x80 == 0 {
            return Ok((i64::from_le_bytes(value.to_le_bytes()), i + 1));
        }
    }
    if buf.len() < MAX_VARLONG_BYTES {
        Err(ProtocolError::Truncated {
            needed: MAX_VARLONG_BYTES - buf.len(),
            available: 0,
        })
    } else {
        Err(ProtocolError::VarIntTooLong {
            max: MAX_VARLONG_BYTES,
        })
    }
}

/// Convenience: encoded bytes of a VarInt.
#[must_use]
pub fn varint_bytes(value: i32) -> Vec<u8> {
    let mut out = Vec::with_capacity(MAX_VARINT_BYTES);
    write_varint(&mut out, value);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_encodings() {
        // Table from wiki.vg / the Minecraft protocol documentation.
        let cases: &[(i32, &[u8])] = &[
            (0, &[0x00]),
            (1, &[0x01]),
            (2, &[0x02]),
            (127, &[0x7f]),
            (128, &[0x80, 0x01]),
            (255, &[0xff, 0x01]),
            (2_097_151, &[0xff, 0xff, 0x7f]),
            (2_147_483_647, &[0xff, 0xff, 0xff, 0xff, 0x07]),
            (-1, &[0xff, 0xff, 0xff, 0xff, 0x0f]),
            (-2_147_483_648, &[0x80, 0x80, 0x80, 0x80, 0x08]),
        ];
        for (value, expected) in cases {
            assert_eq!(&varint_bytes(*value), expected, "encode {value}");
            let (decoded, used) = read_varint(expected).expect("decode");
            assert_eq!(decoded, *value);
            assert_eq!(used, expected.len());
        }
    }

    #[test]
    fn roundtrip_boundaries() {
        let mut values = vec![i32::MIN, i32::MAX, 0, -1, 1];
        let mut v: i32 = 1;
        while let Some(next) = v.checked_mul(3) {
            values.push(v);
            values.push(-v);
            v = next;
        }
        for value in values {
            let bytes = varint_bytes(value);
            assert!(bytes.len() <= MAX_VARINT_BYTES);
            assert_eq!(read_varint(&bytes), Ok((value, bytes.len())));
        }
    }

    #[test]
    fn varlong_roundtrip() {
        for value in [0i64, 1, -1, i64::MAX, i64::MIN, 1_700_000_000_000] {
            let mut bytes = Vec::new();
            write_varlong(&mut bytes, value);
            assert!(bytes.len() <= MAX_VARLONG_BYTES);
            assert_eq!(read_varlong(&bytes), Ok((value, bytes.len())));
        }
    }

    #[test]
    fn truncated_is_error() {
        assert!(matches!(
            read_varint(&[0x80]),
            Err(ProtocolError::Truncated { .. })
        ));
        assert!(matches!(
            read_varint(&[0x80, 0x80, 0x80, 0x80]),
            Err(ProtocolError::Truncated { .. })
        ));
    }

    #[test]
    fn overlong_is_error() {
        // Six continuation bytes: a peer trying to overrun the reader.
        let bomb = [0x80, 0x80, 0x80, 0x80, 0x80, 0x01];
        assert!(matches!(
            read_varint(&bomb),
            Err(ProtocolError::VarIntTooLong { max: 5 })
        ));
    }
}
