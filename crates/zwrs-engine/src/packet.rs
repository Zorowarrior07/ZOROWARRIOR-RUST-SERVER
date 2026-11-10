//! Parsing of the packets the engine actually handles.
//!
//! Only the connection-establishment packets are implemented, because that is
//! exactly what this build serves: handshake, status request/ping, login start
//! and disconnect. Anything else is reported as unsupported instead of being
//! half-implemented.

use zwrs_core::identity::Username;
use zwrs_protocol::{PacketReader, PacketWriter, ProtocolError};

/// Connection state the client asks for in its handshake.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NextState {
    /// Server-list ping.
    Status,
    /// Login.
    Login,
    /// Anything else (transfer in newer versions, or malformed).
    Other(i32),
}

/// The first packet of every modern connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Handshake {
    /// Protocol version the client speaks.
    pub protocol: i32,
    /// Hostname the client used (may include `\0FML\0` style forwarding data).
    pub server_address: String,
    /// Port the client connected to.
    pub server_port: u16,
    /// Requested next state.
    pub next_state: NextState,
}

impl Handshake {
    /// The hostname with any Forge/Bungee forwarding suffix removed.
    #[must_use]
    pub fn clean_address(&self) -> &str {
        self.server_address
            .split('\0')
            .next()
            .unwrap_or(&self.server_address)
    }

    /// Parses a handshake payload (packet id included).
    ///
    /// # Errors
    /// [`ProtocolError`] when the packet id or structure is wrong.
    pub fn parse(payload: &[u8]) -> Result<Self, ProtocolError> {
        let mut r = PacketReader::new(payload);
        let id = r.varint()?;
        if id != 0x00 {
            return Err(ProtocolError::UnknownPacketId(id));
        }
        let protocol = r.varint()?;
        // 255 characters is the vanilla limit for the server-address field.
        let server_address = r.string(255)?;
        let server_port = r.u16()?;
        let state = r.varint()?;
        let next_state = match state {
            1 => NextState::Status,
            2 => NextState::Login,
            other => NextState::Other(other),
        };
        Ok(Self {
            protocol,
            server_address,
            server_port,
            next_state,
        })
    }
}

/// Login Start, in the two shapes that matter for us.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginStart {
    /// Username claimed by the client (not yet authenticated).
    pub raw_username: String,
    /// Parsed username, when it matches vanilla rules.
    pub username: Option<Username>,
}

impl LoginStart {
    /// Parses a Login Start payload.
    ///
    /// Protocol 763+ adds a UUID field and 1.20.2+ slightly changes the layout;
    /// the username is always the first field, so it is read uniformly and any
    /// trailing fields are ignored. The username is validated separately so a
    /// bad name produces a clear disconnect instead of a parse error.
    ///
    /// # Errors
    /// [`ProtocolError`] when the packet id is wrong or the string is unreadable.
    pub fn parse(payload: &[u8]) -> Result<Self, ProtocolError> {
        let mut r = PacketReader::new(payload);
        let id = r.varint()?;
        if id != 0x00 {
            return Err(ProtocolError::UnknownPacketId(id));
        }
        let raw_username = r.string(64)?;
        Ok(Self {
            username: Username::parse(&raw_username).ok(),
            raw_username,
        })
    }
}

/// Status Request (empty payload, id 0x00 in the status state).
///
/// # Errors
/// [`ProtocolError::UnknownPacketId`] for any other id.
pub fn parse_status_request(payload: &[u8]) -> Result<(), ProtocolError> {
    let mut r = PacketReader::new(payload);
    let id = r.varint()?;
    if id == 0x00 {
        Ok(())
    } else {
        Err(ProtocolError::UnknownPacketId(id))
    }
}

/// Ping (id 0x01, payload is an opaque `i64` echoed back).
///
/// # Errors
/// [`ProtocolError`] when the id or payload is wrong.
pub fn parse_ping(payload: &[u8]) -> Result<i64, ProtocolError> {
    let mut r = PacketReader::new(payload);
    let id = r.varint()?;
    if id != 0x01 {
        return Err(ProtocolError::UnknownPacketId(id));
    }
    r.i64()
}

/// Builds a Status Response packet (id 0x00, JSON string), framed.
#[must_use]
pub fn status_response(json: &str) -> Vec<u8> {
    PacketWriter::with_id(0x00).string(json).into_frame()
}

/// Builds a Pong packet (id 0x01, echoed `i64`), framed.
#[must_use]
pub fn pong(payload: i64) -> Vec<u8> {
    PacketWriter::with_id(0x01).i64(payload).into_frame()
}

/// Builds a Login Disconnect packet, framed.
///
/// The message is JSON chat text; ZWRS sends a plain `{"text": ...}` document.
#[must_use]
pub fn login_disconnect(message: &str) -> Vec<u8> {
    // Build the chat component with the JSON writer so newlines, quotes and
    // control characters are escaped correctly.
    let mut component = zwrs_core::Json::object();
    component.insert("text".into(), zwrs_core::Json::String(message.to_string()));
    let json = zwrs_core::Json::Object(component).to_compact();
    PacketWriter::with_id(0x00).string(&json).into_frame()
}

/// Builds the legacy (1.6 and older) `0xFE` response packet.
///
/// Layout: `0xFF`, short length, UTF-16BE string
/// `"§1" + \0 + protocol + version + motd + online + max`.
/// It is what server-list scanners and very old clients expect.
#[must_use]
pub fn legacy_kick_response(protocol: i32, version: &str, motd: &str, online: i32, max: i32) -> Vec<u8> {
    let mut text = String::new();
    text.push_str("\u{a7}1\0");
    text.push_str(&protocol.to_string());
    text.push('\0');
    text.push_str(version);
    text.push('\0');
    text.push_str(motd);
    text.push('\0');
    text.push_str(&online.to_string());
    text.push('\0');
    text.push_str(&max.to_string());

    let mut string_bytes = Vec::with_capacity(text.len() * 2);
    for unit in text.encode_utf16() {
        string_bytes.extend_from_slice(&unit.to_be_bytes());
    }
    // Wire layout: [0xFF][unsigned short length][UTF-16BE string]
    let mut out = Vec::with_capacity(string_bytes.len() + 3);
    out.push(0xFF);
    let len = u16::try_from(string_bytes.len()).unwrap_or(u16::MAX);
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(&string_bytes);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use zwrs_protocol::PacketWriter;

    fn handshake_payload(protocol: i32, host: &str, port: u16, state: i32) -> Vec<u8> {
        PacketWriter::with_id(0x00)
            .varint(protocol)
            .string(host)
            .u16(port)
            .varint(state)
            .into_payload()
    }

    #[test]
    fn handshake_parses_status_and_login() {
        let payload = handshake_payload(763, "mc.example.org", 25565, 1);
        let hs = Handshake::parse(&payload).unwrap();
        assert_eq!(hs.protocol, 763);
        assert_eq!(hs.server_address, "mc.example.org");
        assert_eq!(hs.server_port, 25565);
        assert_eq!(hs.next_state, NextState::Status);

        let payload = handshake_payload(47, "localhost", 25565, 2);
        assert_eq!(Handshake::parse(&payload).unwrap().next_state, NextState::Login);
    }

    #[test]
    fn handshake_strips_forwarding_suffix() {
        let payload = handshake_payload(754, "play.example.org\u{0}FML\u{0}1.16.5", 25565, 2);
        let hs = Handshake::parse(&payload).unwrap();
        assert_eq!(hs.clean_address(), "play.example.org");
    }

    #[test]
    fn handshake_rejects_wrong_packet_id_and_truncation() {
        let mut payload = handshake_payload(763, "host", 25565, 1);
        payload[0] = 0x01;
        assert!(matches!(
            Handshake::parse(&payload),
            Err(ProtocolError::UnknownPacketId(1))
        ));

        let short = handshake_payload(763, "host", 25565, 1);
        assert!(Handshake::parse(&short[..4]).is_err());
        assert!(Handshake::parse(&[]).is_err());
    }

    #[test]
    fn login_start_validates_username() {
        let good = PacketWriter::with_id(0x00).string("Notch").into_payload();
        let ls = LoginStart::parse(&good).unwrap();
        assert_eq!(ls.raw_username, "Notch");
        assert!(ls.username.is_some());

        let bad = PacketWriter::with_id(0x00).string("bad name!").into_payload();
        let ls = LoginStart::parse(&bad).unwrap();
        assert!(ls.username.is_none(), "invalid names must not validate");
        assert_eq!(ls.raw_username, "bad name!");
    }

    #[test]
    fn status_and_ping_roundtrip() {
        let req = PacketWriter::with_id(0x00).into_payload();
        assert_eq!(parse_status_request(&req), Ok(()));

        let ping = PacketWriter::with_id(0x01).i64(0x0123_4567_89ab_cdef).into_payload();
        assert_eq!(parse_ping(&ping), Ok(0x0123_4567_89ab_cdef));

        assert!(parse_ping(&req).is_err());
    }

    #[test]
    fn status_response_is_a_valid_frame() {
        let frame = status_response("{\"a\":1}");
        let (payload, used) = zwrs_protocol::read_frame(&frame).unwrap().unwrap();
        assert_eq!(used, frame.len());
        let mut r = PacketReader::new(payload);
        assert_eq!(r.varint().unwrap(), 0);
        assert_eq!(r.string(4096).unwrap(), "{\"a\":1}");
    }

    #[test]
    fn disconnect_escapes_the_message() {
        let frame = login_disconnect("say \"hi\"\\now");
        let (payload, _) = zwrs_protocol::read_frame(&frame).unwrap().unwrap();
        let mut r = PacketReader::new(payload);
        assert_eq!(r.varint().unwrap(), 0);
        let json = r.string(4096).unwrap();
        assert_eq!(json, "{\"text\":\"say \\\"hi\\\"\\\\now\"}");
        // The produced document must itself be valid JSON.
        assert!(zwrs_core::Json::parse(&json).is_ok());
    }

    #[test]
    fn legacy_response_layout() {
        let bytes = legacy_kick_response(127, "ZWRS", "hi", 1, 20);
        assert_eq!(bytes[0], 0xFF);
        let len = u16::from_be_bytes([bytes[1], bytes[2]]) as usize;
        assert_eq!(len, bytes.len() - 3);
        let text: String = bytes[3..]
            .chunks_exact(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .filter_map(|u| char::from_u32(u32::from(u)).or(if u == 0 { Some('\0') } else { None }))
            .collect();
        assert!(text.starts_with("\u{a7}1\0"));
        assert!(text.contains("ZWRS"));
    }
}
