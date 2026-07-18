//! The hole-punch: drive disco Ping/Pong to open a NAT hole to one peer
//! and report the first address that answers — a validated, bidirectional
//! path we can hand to quinn.
//!
//! The rule set is small (see the spec / the design notes): ping every
//! candidate (opens our NAT toward it); on an inbound Ping, Pong the
//! source, and the *first* time we hear from a peer we haven't validated,
//! Ping it back so both directions get proven even if one ping is lost;
//! on a Pong that matches a Ping we sent, that address is validated.
//!
//! [`PunchState`] is the pure rule set — `tick`/`on_poke` return the pokes
//! to send, no I/O — and [`punch`] is the async shell that sends them and
//! feeds inbound ones from the socket.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::Duration;

use tokio::sync::mpsc::UnboundedReceiver;
use tokio::time::interval;
use tokio::time::sleep;

use super::disco::DiscoKey;
use super::disco::DiscoMsg;
use super::socket::Poke;
use super::socket::PokeSender;

/// How often we re-ping candidates while still trying (iroh's number).
/// The first tick fires immediately, so punching starts at once.
const PING_INTERVAL: Duration = Duration::from_secs(5);

/// The punch rule set for one peer. No I/O: every method returns the
/// pokes the shell should send.
struct PunchState {
    key: DiscoKey,
    /// Addresses to ping — the peer's advertised candidates, plus any
    /// source we hear an inbound Ping from.
    candidates: Vec<SocketAddr>,
    /// tx_id → the address we sent that Ping to, so a matching Pong tells
    /// us which address is reachable.
    sent: HashMap<[u8; 8], SocketAddr>,
    /// First address to answer a Pong. Once set we stop pinging back.
    validated: Option<SocketAddr>,
}

impl PunchState {
    fn new(key: DiscoKey, candidates: Vec<SocketAddr>) -> Self {
        Self { key, candidates, sent: HashMap::new(), validated: None }
    }

    /// Ping every candidate — one round, opens/refreshes our NAT toward
    /// each.
    fn tick(&mut self) -> Vec<Poke> {
        self.candidates.clone().into_iter().map(|addr| (addr, self.ping(addr))).collect()
    }

    /// Handle one inbound poke.
    fn on_poke(&mut self, src: SocketAddr, bytes: &[u8]) -> Vec<Poke> {
        match self.key.open(bytes) {
            Some(DiscoMsg::Ping { tx }) => {
                let mut out = vec![(src, self.key.seal(&DiscoMsg::Pong { tx, seen: src }))];
                let known = self.candidates.contains(&src);
                if !known {
                    self.candidates.push(src);
                }
                // Ping back only the first time we hear from a not-yet-
                // validated peer; after that the tick re-pings it. Gating
                // on `known` stops a ping-back storm if Pongs are lost.
                if self.validated.is_none() && !known {
                    out.push((src, self.ping(src)));
                }
                out
            }
            Some(DiscoMsg::Pong { tx, .. }) => {
                if let Some(addr) = self.sent.remove(&tx) {
                    self.validated.get_or_insert(addr);
                }
                Vec::new()
            }
            // Not our channel, or failed authentication — ignore.
            None => Vec::new(),
        }
    }

    fn ping(&mut self, addr: SocketAddr) -> Vec<u8> {
        let mut tx = [0u8; 8];
        {
            use ed25519_dalek::ed25519::signature::rand_core::OsRng;
            use ed25519_dalek::ed25519::signature::rand_core::RngCore;
            OsRng.fill_bytes(&mut tx);
        }
        self.sent.insert(tx, addr);
        self.key.seal(&DiscoMsg::Ping { tx })
    }
}

/// Punch a hole to `candidates`, returning the first validated address or
/// `None` after `timeout`. Sends pokes via `pokes`; consumes inbound
/// pokes (for this session) from `inbox`.
///
/// Returns as soon as one address validates — that path is bidirectionally
/// open, and the caller (dialer) connects to it while QUIC's own packets
/// keep the hole alive. The accepting side runs this too, purely to open
/// its own NAT, and accepts the incoming connection regardless.
pub async fn punch(
    pokes: &PokeSender,
    inbox: &mut UnboundedReceiver<Poke>,
    key: DiscoKey,
    candidates: Vec<SocketAddr>,
    timeout: Duration,
) -> Option<SocketAddr> {
    let mut state = PunchState::new(key, candidates);
    let mut ticker = interval(PING_INTERVAL);
    let deadline = sleep(timeout);
    tokio::pin!(deadline);

    loop {
        let out = tokio::select! {
            _ = ticker.tick() => state.tick(),
            got = inbox.recv() => match got {
                Some((src, bytes)) => state.on_poke(src, &bytes),
                None => return state.validated, // socket gone
            },
            _ = &mut deadline => return state.validated,
        };
        for (addr, bytes) in out {
            let _ = pokes.send(addr, &bytes).await;
        }
        if state.validated.is_some() {
            return state.validated;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> DiscoKey {
        DiscoKey::new(&[5u8; 32], [6u8; 8])
    }
    fn open_ping(bytes: &[u8]) -> [u8; 8] {
        match key().open(bytes) {
            Some(DiscoMsg::Ping { tx }) => tx,
            other => panic!("expected Ping, got {other:?}"),
        }
    }

    #[test]
    fn tick_pings_every_candidate() {
        let a: SocketAddr = "127.0.0.1:5001".parse().unwrap();
        let b: SocketAddr = "127.0.0.1:5002".parse().unwrap();
        let mut st = PunchState::new(key(), vec![a, b]);
        let pokes = st.tick();
        assert_eq!(pokes.iter().map(|p| p.0).collect::<Vec<_>>(), vec![a, b]);
        // both are real Pings, and both tx_ids are recorded as sent
        for (_, bytes) in &pokes {
            open_ping(bytes);
        }
        assert_eq!(st.sent.len(), 2);
    }

    #[test]
    fn matching_pong_validates() {
        let peer: SocketAddr = "127.0.0.1:5001".parse().unwrap();
        let mut st = PunchState::new(key(), vec![peer]);
        let tx = open_ping(&st.tick()[0].1);

        let pong = key().seal(&DiscoMsg::Pong { tx, seen: "127.0.0.1:9".parse().unwrap() });
        let out = st.on_poke(peer, &pong);
        assert!(out.is_empty());
        assert_eq!(st.validated, Some(peer));

        // an unknown tx does not validate
        let mut st2 = PunchState::new(key(), vec![peer]);
        let stray = key().seal(&DiscoMsg::Pong { tx: [0; 8], seen: peer });
        st2.on_poke(peer, &stray);
        assert_eq!(st2.validated, None);
    }

    #[test]
    fn inbound_ping_pongs_then_pings_back_once() {
        let mut st = PunchState::new(key(), vec![]);
        let src: SocketAddr = "127.0.0.1:6000".parse().unwrap();
        let ping = key().seal(&DiscoMsg::Ping { tx: [7; 8] });

        // first contact: Pong (echoing tx) + one ping-back; src is learned
        let out = st.on_poke(src, &ping);
        assert_eq!(out.len(), 2);
        assert!(matches!(key().open(&out[0].1), Some(DiscoMsg::Pong { tx, .. }) if tx == [7; 8]));
        open_ping(&out[1].1);
        assert!(st.candidates.contains(&src));

        // second ping from the same src: Pong only, no ping-back storm
        let out = st.on_poke(src, &ping);
        assert_eq!(out.len(), 1);
        assert!(matches!(key().open(&out[0].1), Some(DiscoMsg::Pong { .. })));
    }

    #[test]
    fn validated_ping_does_not_ping_back() {
        let peer: SocketAddr = "127.0.0.1:5001".parse().unwrap();
        let mut st = PunchState::new(key(), vec![peer]);
        let tx = open_ping(&st.tick()[0].1);
        st.on_poke(peer, &key().seal(&DiscoMsg::Pong { tx, seen: peer }));
        assert!(st.validated.is_some());

        // new peer pings after we're validated → Pong only
        let other: SocketAddr = "127.0.0.1:7000".parse().unwrap();
        let out = st.on_poke(other, &key().seal(&DiscoMsg::Ping { tx: [1; 8] }));
        assert_eq!(out.len(), 1);
        assert!(matches!(key().open(&out[0].1), Some(DiscoMsg::Pong { .. })));
    }
}
