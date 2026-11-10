//! ZWRS Minecraft Java Edition protocol primitives.
//!
//! # Scope
//!
//! This crate implements the wire-level pieces ZWRS itself relies on for the
//! connection lifecycle it actually serves:
//!
//! * VarInt / VarLong integer encoding
//! * length-prefixed packet framing (both the pre-1.16 "packet length" layout
//!   and the modern layout, which are identical on the wire)
//! * handshake, status request/response, ping/pong (all supported versions)
//! * login start and disconnect for protocol 47 (1.8.x) and 754 (1.16.5)
//! * the legacy `0xFE` server-list ping, still used by 1.6-era clients and by
//!   some server-list scanners
//!
//! Full world/gameplay support for every version is **not** implemented here;
//! see `docs/COMPATIBILITY.md` for what is verified and what is not. The engine
//! reports its real capabilities rather than pretending to be a complete server.

#![deny(missing_docs)]

mod frame;
mod varint;

pub use frame::{read_frame, write_frame, PacketReader, PacketWriter, MAX_PACKET_LEN};
pub use varint::{
    read_varint, read_varlong, varint_bytes, write_varint, write_varlong, MAX_VARINT_BYTES,
};

/// Errors produced while parsing or writing protocol data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtocolError {
    /// The buffer ended before the structure was complete.
    Truncated {
        /// Bytes still required.
        needed: usize,
        /// Bytes available.
        available: usize,
    },
    /// A VarInt/VarLong used more bytes than the protocol allows.
    VarIntTooLong {
        /// Maximum permitted length.
        max: usize,
    },
    /// A declared string length exceeds the protocol maximum.
    StringTooLong {
        /// Length claimed on the wire.
        claimed: usize,
        /// Maximum permitted.
        max: usize,
    },
    /// A string was not valid UTF-8.
    InvalidUtf8,
    /// A packet declared a length outside the accepted range.
    BadLength {
        /// Declared packet or frame length.
        claimed: usize,
        /// Human-readable reason.
        reason: &'static str,
    },
    /// The packet id was not understood in the current state.
    UnknownPacketId(i32),
    /// The payload did not match the expected structure.
    Malformed(&'static str),
}

impl std::fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Truncated { needed, available } => {
                write!(f, "truncated packet: needed {needed} more bytes, {available} available")
            }
            Self::VarIntTooLong { max } => write!(f, "varint exceeded {max} bytes"),
            Self::StringTooLong { claimed, max } => {
                write!(f, "string length {claimed} exceeds maximum {max}")
            }
            Self::InvalidUtf8 => f.write_str("invalid utf-8 in protocol string"),
            Self::BadLength { claimed, reason } => {
                write!(f, "bad length {claimed}: {reason}")
            }
            Self::UnknownPacketId(id) => write!(f, "unknown packet id 0x{id:x}"),
            Self::Malformed(m) => write!(f, "malformed packet: {m}"),
        }
    }
}

impl std::error::Error for ProtocolError {}

/// Minecraft Java protocol version numbers relevant to ZWRS.
pub mod versions {
    /// 1.8.x — the oldest client family ZWRS targets.
    pub const V1_8: i32 = 47;
    /// 1.12.2 — last of the "pre-flattening" formats.
    pub const V1_12_2: i32 = 340;
    /// 1.16.5 — long-term modded baseline.
    pub const V1_16_5: i32 = 754;
    /// 1.20.1.
    pub const V1_20_1: i32 = 763;
    /// 1.21.1.
    pub const V1_21_1: i32 = 767;
    /// 1.21.4.
    pub const V1_21_4: i32 = 769;

    /// Human-readable name for a protocol version ZWRS knows about.
    #[must_use]
    pub const fn name(proto: i32) -> Option<&'static str> {
        match proto {
            47 => Some("1.8.x"),
            107 => Some("1.9"),
            110 => Some("1.9.4"),
            210 => Some("1.10.x"),
            315 => Some("1.11.x"),
            335 => Some("1.12"),
            338 => Some("1.12.1"),
            340 => Some("1.12.2"),
            393 => Some("1.13"),
            401 => Some("1.13.1"),
            404 => Some("1.13.2"),
            477 => Some("1.14"),
            480 => Some("1.14.1"),
            485 => Some("1.14.2"),
            490 => Some("1.14.3"),
            498 => Some("1.14.4"),
            573 => Some("1.15"),
            575 => Some("1.15.1"),
            578 => Some("1.15.2"),
            735 => Some("1.16"),
            736 => Some("1.16.1"),
            751 => Some("1.16.2"),
            753 => Some("1.16.3"),
            754 => Some("1.16.4/1.16.5"),
            755 => Some("1.17"),
            756 => Some("1.17.1"),
            757 => Some("1.18/1.18.1"),
            758 => Some("1.18.2"),
            759 => Some("1.19"),
            760 => Some("1.19.1/1.19.2"),
            761 => Some("1.19.3"),
            762 => Some("1.19.4"),
            763 => Some("1.20/1.20.1"),
            764 => Some("1.20.2"),
            765 => Some("1.20.3/1.20.4"),
            766 => Some("1.20.5/1.20.6"),
            767 => Some("1.21/1.21.1"),
            768 => Some("1.21.2/1.21.3"),
            769 => Some("1.21.4"),
            770 => Some("1.21.5"),
            771 => Some("1.21.6"),
            772 => Some("1.21.7/1.21.8"),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_names_cover_targets() {
        assert_eq!(versions::name(47), Some("1.8.x"));
        assert_eq!(versions::name(754), Some("1.16.4/1.16.5"));
        assert_eq!(versions::name(763), Some("1.20/1.20.1"));
        assert_eq!(versions::name(999_999), None);
    }

    #[test]
    fn error_display_is_informative() {
        let e = ProtocolError::StringTooLong { claimed: 40_000, max: 32_767 };
        assert!(e.to_string().contains("40000"));
    }
}
