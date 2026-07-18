//! Client↔relay hole-punch assist wire: STUN address echo + a blind TURN
//! datagram bridge. Shared by the relay (server) and libcore (client).
//!
//! Framing is `MAGIC | tag | ...`. `MAGIC`'s first byte clears the QUIC
//! fixed-bit (`0x40`) so one of these never parses as a QUIC packet, and it
//! differs from libcore's disco `MAGIC` (`.p2p`) so the client's P2P socket
//! can split disco, relay-assist, and QUIC on the one port.
//!
//! STUN control (`StunReq`/`StunResp`) is plaintext — the relay can't hold
//! the per-peer MLS key. TURN payloads are the peers' own QUIC, opaque to
//! the relay; the 16-byte token (MLS-derived, secret) both names the bridge
//! and gates it, since only the two peers can derive it.

use std::net::IpAddr;
use std::net::Ipv4Addr;
use std::net::Ipv6Addr;
use std::net::SocketAddr;

/// Tags a relay-assist datagram. First byte (`0x2e`) clears the QUIC
/// fixed-bit; the whole prefix differs from disco's `.p2p`.
pub const MAGIC: [u8; 4] = [0x2e, 0x70, 0x52, 0x72]; // ".pRr"

const TAG_STUN_REQ: u8 = 1;
const TAG_STUN_RESP: u8 = 2;
const TAG_TURN_ALLOC: u8 = 3;
const TAG_TURN_DATA: u8 = 4;

/// TURN bridge token: 16 secret bytes both peers derive from their MLS
/// group, naming (and gating) one bridge.
pub const TOKEN_LEN: usize = 16;

/// `MAGIC | tag`.
const HDR: usize = MAGIC.len() + 1;

/// One relay-assist datagram. `TurnData`'s payload borrows the input so the
/// relay can forward it without a copy.
#[derive(Debug, PartialEq, Eq)]
pub enum RelayMsg<'a> {
    /// Client → relay: "what public address does this socket map to?"
    StunReq { tx: [u8; 8] },
    /// Relay → client: the source address the relay observed for the query.
    StunResp { tx: [u8; 8], seen: SocketAddr },
    /// Client → relay: register this socket as one end of `token`'s bridge.
    TurnAlloc { token: [u8; TOKEN_LEN] },
    /// Client ↔ relay ↔ client: a QUIC datagram to forward to the other end
    /// of `token`'s bridge, carried verbatim.
    TurnData { token: [u8; TOKEN_LEN], payload: &'a [u8] },
}

impl RelayMsg<'_> {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(HDR + 32);
        out.extend_from_slice(&MAGIC);
        match self {
            RelayMsg::StunReq { tx } => {
                out.push(TAG_STUN_REQ);
                out.extend_from_slice(tx);
            },
            RelayMsg::StunResp { tx, seen } => {
                out.push(TAG_STUN_RESP);
                out.extend_from_slice(tx);
                put_addr(&mut out, *seen);
            },
            RelayMsg::TurnAlloc { token } => {
                out.push(TAG_TURN_ALLOC);
                out.extend_from_slice(token);
            },
            RelayMsg::TurnData { token, payload } => {
                out.push(TAG_TURN_DATA);
                out.extend_from_slice(token);
                out.extend_from_slice(payload);
            },
        }
        out
    }

    /// Parse a datagram, or `None` if it isn't relay-assist framed (e.g. a
    /// QUIC packet). A `TurnData` result borrows `pkt`.
    pub fn decode(pkt: &[u8]) -> Option<RelayMsg<'_>> {
        if pkt.len() < HDR || !pkt.starts_with(&MAGIC) {
            return None;
        }
        let body = &pkt[HDR..];
        match pkt[MAGIC.len()] {
            TAG_STUN_REQ => Some(RelayMsg::StunReq { tx: body.get(..8)?.try_into().ok()? }),
            TAG_STUN_RESP => Some(RelayMsg::StunResp {
                tx:   body.get(..8)?.try_into().ok()?,
                seen: get_addr(body.get(8..)?)?,
            }),
            TAG_TURN_ALLOC => {
                Some(RelayMsg::TurnAlloc { token: body.get(..TOKEN_LEN)?.try_into().ok()? })
            },
            TAG_TURN_DATA => Some(RelayMsg::TurnData {
                token:   body.get(..TOKEN_LEN)?.try_into().ok()?,
                payload: &body[TOKEN_LEN..],
            }),
            _ => None,
        }
    }
}

/// Cheap check: does this datagram carry the relay-assist `MAGIC`? Lets a
/// socket split assist from QUIC without a full decode.
pub fn is_assist(pkt: &[u8]) -> bool {
    pkt.len() >= HDR && pkt.starts_with(&MAGIC)
}

/// `port(2, be) | family(1) | ip(4 or 16)`.
fn put_addr(out: &mut Vec<u8>, addr: SocketAddr) {
    out.extend_from_slice(&addr.port().to_be_bytes());
    match addr.ip() {
        IpAddr::V4(v4) => {
            out.push(4);
            out.extend_from_slice(&v4.octets());
        },
        IpAddr::V6(v6) => {
            out.push(6);
            out.extend_from_slice(&v6.octets());
        },
    }
}

fn get_addr(b: &[u8]) -> Option<SocketAddr> {
    let port = u16::from_be_bytes(b.get(..2)?.try_into().ok()?);
    let (fam, ip) = b.get(2..)?.split_first()?;
    match fam {
        4 => {
            let o: [u8; 4] = ip.get(..4)?.try_into().ok()?;
            Some((Ipv4Addr::from(o), port).into())
        },
        6 => {
            let o: [u8; 16] = ip.get(..16)?.try_into().ok()?;
            Some((Ipv6Addr::from(o), port).into())
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(msg: RelayMsg) {
        let bytes = msg.encode();
        assert_eq!(RelayMsg::decode(&bytes), Some(msg));
    }

    #[test]
    fn every_variant_roundtrips() {
        roundtrip(RelayMsg::StunReq { tx: [1; 8] });
        roundtrip(RelayMsg::StunResp { tx: [2; 8], seen: "1.2.3.4:5".parse().unwrap() });
        roundtrip(RelayMsg::StunResp { tx: [3; 8], seen: "[2409:41::9]:443".parse().unwrap() });
        roundtrip(RelayMsg::TurnAlloc { token: [4; TOKEN_LEN] });
        roundtrip(RelayMsg::TurnData { token: [5; TOKEN_LEN], payload: b"opaque quic" });
    }

    #[test]
    fn turn_data_payload_borrows_verbatim() {
        let bytes = RelayMsg::TurnData { token: [7; TOKEN_LEN], payload: b"\xc0abc" }.encode();
        match RelayMsg::decode(&bytes) {
            Some(RelayMsg::TurnData { token, payload }) => {
                assert_eq!(token, [7; TOKEN_LEN]);
                assert_eq!(payload, b"\xc0abc");
            },
            other => panic!("expected TurnData, got {other:?}"),
        }
    }

    #[test]
    fn non_assist_is_rejected() {
        // QUIC-shaped (fixed-bit set) and short junk are not relay-assist.
        assert_eq!(RelayMsg::decode(&[0xc0, 1, 2, 3, 4, 5]), None);
        assert_eq!(RelayMsg::decode(b".pRr"), None); // magic but no tag/body
        assert_eq!(RelayMsg::decode(b""), None);
    }
}
