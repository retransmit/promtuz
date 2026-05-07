//! Iterative `FindNode` / `FindValue` walks with α=3 parallelism and
//! per-hop hedging.
//!
//! ## Algorithm (§4.1)
//!
//! Per design-doc §4.1, we maintain three logical sets:
//!
//! - **`pending`**: the candidate shortlist of peers we *might* query, sorted
//!   by XOR distance to `target`.
//! - **`in_flight`**: the peers we've sent a request to and are still
//!   waiting for. Bounded at `α = 3`.
//! - **`queried`**: peers that have already responded (or been hedged-out).
//!
//! Termination per §4.3:
//! 1. We've contacted the K strictly-closest peers in `pending` and none
//!    returns a closer-than-current peer, OR
//! 2. `LOOKUP_MAX_HOPS` exceeded, OR
//! 3. `LOOKUP_RPC_TIMEOUT_MS` total wall-clock elapsed.
//!
//! ## Hedging (§4.1, second paragraph)
//!
//! When a request hasn't returned within `LOOKUP_HEDGE_MS`, we *don't*
//! cancel it — instead we fire a duplicate to the next-best candidate.
//! Whichever responds first wins; the loser's reply (if it eventually
//! arrives) is folded back into the candidate pool opportunistically.
//!
//! ## Lock contract
//!
//! Like the rest of `dht/`, we never hold a `parking_lot` guard across
//! `await`. The `routing.read().find_closest(...)` call is the only
//! routing-table read; we clone the descriptors out and release the
//! lock before any I/O.
//!
//! design-doc: §4 (lookup protocol), §4.1 (FindNode iterative algorithm),
//! §4.2 (FindValue), §4.3 (termination), §4.4 (Sybil cross-check),
//! §4.5 (why clients don't iterate).

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use common::proto::dht_p2p::DhtPacket;
use common::proto::dht_p2p::DhtRequest;
use common::proto::dht_p2p::DhtResponse;
use common::proto::dht_p2p::FindNode;
use common::proto::dht_p2p::FindValue;
use common::proto::dht_p2p::FindValueOutcome as WireFindValueOutcome;
use common::proto::dht_p2p::NodeDescriptor;
use common::proto::dht_p2p::PresenceRecord;
use common::proto::pack::Packer;
use common::proto::pack::Unpacker;
use common::quic::id::NodeId;
use common::warn;
use quinn::Connection;
use thiserror::Error;
use tokio::time::timeout;

use super::Dht;
use super::config::ALPHA;
use super::config::K;
use super::config::LOOKUP_HEDGE_MS;
use super::config::LOOKUP_MAX_HOPS;
use super::config::LOOKUP_RPC_TIMEOUT_MS;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Outcome of an iterative `FindValue` walk.
///
/// design-doc: §4.2 (Found / NotPresent / Closer collapsed into a
/// caller-friendly trichotomy).
//
// Variant size: `Found(PresenceRecord)` is ~250 B while `NotPresent` is
// zero-sized. Boxing `PresenceRecord` would shrink the enum but every
// caller in the lookup path then needs an extra deref; phase 1f revisits
// once the access pattern is concrete.
#[allow(clippy::large_enum_variant)]
#[derive(Debug)]
pub enum FindValueOutcome {
    /// We collected at least one `Found(record)` reply. The returned
    /// record is the highest-priority winner under §5.3 ordering.
    Found(PresenceRecord),

    /// All reachable closest peers reported `NotPresent`. Authoritative
    /// "user is offline" per §4.2.
    NotPresent,
}

/// Failure modes for an iterative lookup.
#[derive(Debug, Error)]
pub enum LookupError {
    /// Routing table empty — bootstrap has not yet completed.
    #[error("lookup: no candidates in routing table (bootstrap not done?)")]
    NoCandidates,

    /// Total wall-clock budget exhausted before convergence.
    #[error("lookup: timed out after {LOOKUP_RPC_TIMEOUT_MS}ms")]
    Timeout,

    /// `LOOKUP_MAX_HOPS` exceeded without termination — typically means a
    /// network partition or hostile peers feeding loops.
    #[error("lookup: exceeded {LOOKUP_MAX_HOPS} hops")]
    MaxHopsExceeded,

    /// Failed to open a peer connection (TLS, handshake, DNS). Surfaced
    /// only when *every* candidate failed — a single peer failure is
    /// folded into the iteration silently.
    #[error("lookup: peer connect failed: {0}")]
    PeerConnect(#[source] anyhow::Error),
}

// ---------------------------------------------------------------------------
// Internal candidate state
// ---------------------------------------------------------------------------

/// Candidate peer in the lookup shortlist, decorated with its XOR
/// distance to the lookup target so we can sort cheaply.
#[derive(Clone, Debug)]
struct Candidate {
    desc:     NodeDescriptor,
    distance: [u8; 32],
}

fn xor32(a: &[u8; 32], b: &[u8; 32]) -> [u8; 32] {
    let mut out = [0u8; 32];
    for i in 0..32 {
        out[i] = a[i] ^ b[i];
    }
    out
}

fn distance(target: &[u8; 32], peer: &NodeId) -> [u8; 32] {
    xor32(target, peer.as_bytes())
}

// ---------------------------------------------------------------------------
// Connection management
// ---------------------------------------------------------------------------

/// Open (or reuse) a QUIC connection to `peer`. Cached in `dht.peer_conns`.
///
/// On any cached-but-dead connection, we evict and re-dial. Drops are
/// cheap because the inner `quinn::Connection` is `Arc`-shared internally.
///
/// Visible to the rest of `dht/` (notably `publish.rs`) so the cache +
/// dial path is shared rather than duplicated.
pub(crate) async fn connect_to_peer(
    dht: &Arc<Dht>, peer: &NodeDescriptor,
) -> anyhow::Result<Connection> {
    // Fast path: hit the cache.
    if let Some(conn) = dht.peer_conns.read().get(&peer.id).cloned() {
        // If the cached connection is dead, fall through to reconnect.
        if conn.close_reason().is_none() {
            return Ok(conn);
        }
    }

    // Slow path: open a fresh QUIC connection using the dialer machinery
    // wired up in `Dht::attach_dialer`. The `peer_client_cfg` carries
    // ALPN `peer/1` so the responder routes us into the DHT handler.
    //
    // The `Option<...>` shape on `endpoint`/`peer_client_cfg` exists so
    // the unit tests can build a `Dht` without a live QUIC stack — in
    // those tests, `connect_to_peer` is never reached. In production,
    // `Relay::new` always calls `attach_dialer`.
    let endpoint = match dht.endpoint.as_ref() {
        Some(ep) => ep.clone(),
        None => return Err(anyhow::anyhow!("DHT has no endpoint configured")),
    };
    let client_cfg = match dht.peer_client_cfg.as_ref() {
        Some(cfg) => cfg.clone(),
        None => return Err(anyhow::anyhow!("DHT has no peer_client_cfg configured")),
    };

    // SNI string: the peer's NodeId (base32-encoded). The relay's TLS
    // certs use the NodeId as the SNI/CN per `common/src/bin/certgen.rs`.
    let sni = peer.id.to_string();
    let conn = endpoint
        .connect_with(client_cfg.as_ref().clone(), peer.addr, &sni)?
        .await?;

    // Cache. Race: another task may have raced ahead with a connection
    // to the same peer; if so, drop the loser. Both `Connection`s are
    // independently usable — the eventual consistency is only about
    // *which* one future calls reuse.
    {
        let mut conns = dht.peer_conns.write();
        if let Some(existing) = conns.get(&peer.id).cloned() {
            if existing.close_reason().is_none() {
                return Ok(existing);
            }
        }
        conns.insert(peer.id, conn.clone());
    }
    dht.metrics.inc_peer_conns_opened();
    Ok(conn)
}

/// Send one DHT request over a fresh bi-stream and read the response.
///
/// Per §2.2, a DHT RPC is one bi-stream: open_bi → write request →
/// finish() the send side → read length-prefixed response → done.
///
/// Wraps the entire round-trip in a `LOOKUP_RPC_TIMEOUT_MS` deadline so
/// a single slow peer can't stall the iteration past its budget.
async fn rpc_one(conn: &Connection, req: DhtRequest) -> anyhow::Result<DhtResponse> {
    let pkt = DhtPacket::Request(req);
    let bytes = pkt.pack()?;

    let (mut send, mut recv) = conn.open_bi().await?;
    send.write_all(&bytes).await?;
    send.finish()?;

    let resp = DhtPacket::unpack(&mut recv).await?;
    match resp {
        DhtPacket::Response(r) => Ok(r),
        DhtPacket::Request(_) => {
            Err(anyhow::anyhow!("rpc_one: peer sent a Request where a Response was expected"))
        }
    }
}

// ---------------------------------------------------------------------------
// FindNode iterative walk
// ---------------------------------------------------------------------------

/// Iterative `FindNode` walk to discover the k closest peers to `target`.
///
/// Used by:
/// - bootstrap's "self-FindNode" forced-convergence step (§3.5),
/// - the publish path to find STORE recipients (§5.2),
/// - bucket-refresh to re-discover stale ranges (§3.2).
///
/// Returns the top-k peers by XOR distance the walk converged on.
///
/// design-doc: §4.1.
pub(crate) async fn lookup_node(
    dht: Arc<Dht>, target: NodeId,
) -> Result<Vec<NodeDescriptor>, LookupError> {
    dht.metrics.inc_lookups_started();

    let target_bytes = *target.as_bytes();

    // Seed the shortlist from the routing table. No `await` between
    // taking the read guard and dropping it — clone descriptors out.
    let initial: Vec<NodeDescriptor> = {
        let routing = dht.routing.read();
        routing.find_closest(&target, K * 2)
    };

    if initial.is_empty() {
        dht.metrics.inc_lookups_failed();
        return Err(LookupError::NoCandidates);
    }

    // The "shortlist" — kept sorted by distance ascending.
    let mut candidates: Vec<Candidate> = initial
        .into_iter()
        .map(|desc| {
            let distance = distance(&target_bytes, &desc.id);
            Candidate { desc, distance }
        })
        .collect();
    candidates.sort_by(|a, b| a.distance.cmp(&b.distance));

    let mut queried: HashSet<NodeId> = HashSet::new();
    queried.insert(dht.node_id); // never query self
    let mut closest_so_far: Vec<Candidate> = Vec::with_capacity(K);

    let deadline = Instant::now() + Duration::from_millis(LOOKUP_RPC_TIMEOUT_MS);
    let mut hops: u32 = 0;

    let res = run_iterative_loop(
        &dht,
        &target_bytes,
        &mut candidates,
        &mut queried,
        &mut closest_so_far,
        &mut hops,
        deadline,
        false, // is_value_lookup = false
    )
    .await;

    match res {
        Ok(_) => {
            dht.metrics.inc_lookups_succeeded();
            Ok(closest_so_far.into_iter().take(K).map(|c| c.desc).collect())
        }
        Err(IterError::ValueFound(_)) => {
            // Cannot happen — value lookup is disabled for this call.
            unreachable!("FindNode walk surfaced a ValueFound branch")
        }
        Err(IterError::Lookup(e)) => {
            dht.metrics.inc_lookups_failed();
            Err(e)
        }
    }
}

// ---------------------------------------------------------------------------
// FindValue iterative walk
// ---------------------------------------------------------------------------

/// Iterative `FindValue` walk for `user_ipk`.
///
/// Same structure as `lookup_node` but each hop sends `FindValue` and
/// honours `Found` / `NotPresent` / `Closer` per §4.2.
///
/// Quorum behaviour:
/// - The first `Found(record)` ends the iteration immediately and is
///   returned (§4.2 — "return the record up the lookup stack").
/// - If we've queried K peers and none returned `Found`, the result is
///   `NotPresent` (§4.4 cross-check: K-of-K NotPresent => authoritative).
/// - **§4.4 sybil/eclipse mitigation:** if we collected at least 1 hit
///   alongside K-1 `NotPresent`, this *could* be honest (record was
///   just published) or hostile (eclipse). For v1 we log a warning and
///   prefer the `Found` reply; phase 1h hardens with a quorum-of-2
///   policy.
///
/// design-doc: §4.2, §4.4.
pub(crate) async fn lookup_value(
    dht: Arc<Dht>, user_ipk: [u8; 32],
) -> Result<FindValueOutcome, LookupError> {
    dht.metrics.inc_lookups_started();

    // Bootstrap the candidate set the same way as `lookup_node`.
    let target_id = NodeId::from_bytes(user_ipk);

    let initial: Vec<NodeDescriptor> = {
        let routing = dht.routing.read();
        routing.find_closest(&target_id, K * 2)
    };

    if initial.is_empty() {
        dht.metrics.inc_lookups_failed();
        return Err(LookupError::NoCandidates);
    }

    let mut candidates: Vec<Candidate> = initial
        .into_iter()
        .map(|desc| {
            let distance = distance(&user_ipk, &desc.id);
            Candidate { desc, distance }
        })
        .collect();
    candidates.sort_by(|a, b| a.distance.cmp(&b.distance));

    let mut queried: HashSet<NodeId> = HashSet::new();
    queried.insert(dht.node_id);
    let mut closest_so_far: Vec<Candidate> = Vec::with_capacity(K);

    let deadline = Instant::now() + Duration::from_millis(LOOKUP_RPC_TIMEOUT_MS);
    let mut hops: u32 = 0;

    let res = run_iterative_loop(
        &dht,
        &user_ipk,
        &mut candidates,
        &mut queried,
        &mut closest_so_far,
        &mut hops,
        deadline,
        true, // is_value_lookup = true
    )
    .await;

    match res {
        Ok(()) => {
            // Walk completed without surfacing a `Found`. K closest
            // (or what we could reach) all said `NotPresent`/`Closer`.
            dht.metrics.inc_lookups_succeeded();
            Ok(FindValueOutcome::NotPresent)
        }
        Err(IterError::ValueFound(record)) => {
            dht.metrics.inc_lookups_succeeded();
            Ok(FindValueOutcome::Found(*record))
        }
        Err(IterError::Lookup(e)) => {
            dht.metrics.inc_lookups_failed();
            Err(e)
        }
    }
}

// ---------------------------------------------------------------------------
// Shared iterative loop
// ---------------------------------------------------------------------------

/// Internal control-flow error from `run_iterative_loop`. Splits the
/// "caller-visible failure" from the "found-the-value, exit early"
/// signal so `lookup_value` can return on the first hit without doing
/// extra hops.
enum IterError {
    /// A `Found(record)` arrived during a value lookup; abort the loop
    /// and ship this record back to the caller.
    ValueFound(Box<PresenceRecord>),
    /// A genuine lookup failure (timeout, max-hops, etc.).
    Lookup(LookupError),
}

/// Drive the α-parallel iterative loop with hedging. Used by both
/// `lookup_node` (with `is_value_lookup = false`) and `lookup_value`
/// (with `is_value_lookup = true`).
///
/// Returns `Ok(())` when the walk converged peacefully (closest_so_far
/// is now populated), `Err(IterError::ValueFound)` for an early-exit
/// value hit, or `Err(IterError::Lookup(_))` for failures.
async fn run_iterative_loop(
    dht: &Arc<Dht>, target: &[u8; 32], candidates: &mut Vec<Candidate>,
    queried: &mut HashSet<NodeId>, closest_so_far: &mut Vec<Candidate>, hops: &mut u32,
    deadline: Instant, is_value_lookup: bool,
) -> Result<(), IterError> {
    use tokio::task::JoinSet;

    let mut not_present_count: usize = 0;

    loop {
        if Instant::now() >= deadline {
            return Err(IterError::Lookup(LookupError::Timeout));
        }
        if *hops >= LOOKUP_MAX_HOPS {
            return Err(IterError::Lookup(LookupError::MaxHopsExceeded));
        }

        // 1. Snapshot the next α candidates that haven't been queried.
        let mut batch: Vec<NodeDescriptor> = Vec::with_capacity(ALPHA);
        for c in candidates.iter() {
            if !queried.contains(&c.desc.id) {
                batch.push(c.desc.clone());
                if batch.len() >= ALPHA {
                    break;
                }
            }
        }
        if batch.is_empty() {
            // Nothing left to query — loop has converged.
            return Ok(());
        }

        // 2. Fire α requests in parallel. Each is bounded by the
        //    `LOOKUP_HEDGE_MS` (per-hop hedging) and the overall deadline.
        let mut set: JoinSet<RpcResult> = JoinSet::new();
        for desc in batch.iter() {
            queried.insert(desc.id);
            let dht_ref = dht.clone();
            let desc_clone = desc.clone();
            let target_arr = *target;
            let is_value = is_value_lookup;
            set.spawn(async move {
                send_one_hop(&dht_ref, desc_clone, target_arr, is_value).await
            });
        }

        // 3. Drain results. The first reply that arrives unblocks the
        //    next batch; later replies still inform `closest_so_far`.
        //
        // Control flow:
        // - Keep calling `set.join_next()` to harvest replies.
        // - The whole drain is bounded by `deadline`; if we've already
        //   got `>=1` reply we additionally bound subsequent waits by
        //   `LOOKUP_HEDGE_MS` so the next batch can fire (the hedging
        //   that §4.1 paragraph 2 describes).
        let hedge_window = Duration::from_millis(LOOKUP_HEDGE_MS);
        let mut got_one = false;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                // Out of overall budget. Abort outstanding tasks and
                // surface the timeout.
                set.abort_all();
                return Err(IterError::Lookup(LookupError::Timeout));
            }
            // Smaller of: hedge window if we already got something, or
            // the remaining wall-clock budget.
            let wait = if got_one { hedge_window.min(remaining) } else { remaining };

            let joined = match timeout(wait, set.join_next()).await {
                Ok(j) => j,
                Err(_) => {
                    // Hit the wait window without a new reply. If
                    // we've already harvested at least one, leave the
                    // outstanding tasks running and break to fire the
                    // next batch (the hedging behaviour). Otherwise
                    // we're still waiting on the very first reply and
                    // the next loop iter will re-check the deadline.
                    if got_one {
                        break;
                    } else {
                        continue;
                    }
                }
            };
            let Some(task_result) = joined else {
                // No more in-flight tasks — break to advance the loop.
                break;
            };
            let result = match task_result {
                Ok(r) => r,
                Err(_join) => continue, // task panicked / was aborted
            };
            got_one = true;
            match result {
                RpcResult::FindNodeReply(closer) => {
                    integrate_descriptors(target, candidates, &closer);
                }
                RpcResult::FindValueClose(closer) => {
                    integrate_descriptors(target, candidates, &closer);
                }
                RpcResult::FindValueFound(record) => {
                    // Eclipse mitigation per §4.4: if we *already*
                    // observed K-1 NotPresent and now get a Found,
                    // log a warning. Otherwise the Found is the
                    // canonical answer.
                    if not_present_count >= K.saturating_sub(1) {
                        warn!(
                            "DHT lookup_value: {} NotPresent + 1 Found — possible eclipse \
                             attempt, continuing with the Found reply",
                            not_present_count
                        );
                    }
                    set.abort_all();
                    return Err(IterError::ValueFound(Box::new(record)));
                }
                RpcResult::FindValueNotPresent => {
                    not_present_count += 1;
                }
                RpcResult::Failed => {
                    // Peer error — fold silently. The peer remains in
                    // `queried` so we don't re-try it within this
                    // walk; routing-table-level eviction handles
                    // permanent failures.
                }
            }
        }

        *hops += 1;

        // 4. Update `closest_so_far` from the candidate pool.
        candidates.sort_by(|a, b| a.distance.cmp(&b.distance));
        closest_so_far.clear();
        for c in candidates.iter().take(K) {
            closest_so_far.push(c.clone());
        }

        // 5. Termination: if no unqueried peer is closer than the
        //    farthest entry in closest_so_far, we're done.
        let farthest = closest_so_far.last().map(|c| c.distance);
        let any_closer_unqueried = candidates.iter().any(|c| {
            !queried.contains(&c.desc.id)
                && farthest.map(|f| c.distance < f).unwrap_or(true)
        });
        if !any_closer_unqueried {
            // No peer not-yet-queried is closer than the K-th best —
            // standard Kademlia termination per §4.3 case (1)/(2).
            return Ok(());
        }
    }
}

/// One-hop RPC outcome. We collapse the wire `DhtResponse` into a small
/// enum for the iterative loop's match arm.
enum RpcResult {
    FindNodeReply(Vec<NodeDescriptor>),
    FindValueClose(Vec<NodeDescriptor>),
    FindValueFound(PresenceRecord),
    FindValueNotPresent,
    Failed,
}

/// Connect (or reuse) and issue one DHT RPC against `peer`.
async fn send_one_hop(
    dht: &Arc<Dht>, peer: NodeDescriptor, target: [u8; 32], is_value_lookup: bool,
) -> RpcResult {
    let conn = match connect_to_peer(dht, &peer).await {
        Ok(c) => c,
        Err(_) => return RpcResult::Failed,
    };

    let req = if is_value_lookup {
        DhtRequest::FindValue(FindValue {
            user_ipk:  target.into(),
            requester: dht.node_id,
        })
    } else {
        DhtRequest::FindNode(FindNode {
            target:    target.into(),
            requester: dht.node_id,
        })
    };

    let resp = match timeout(
        Duration::from_millis(LOOKUP_RPC_TIMEOUT_MS),
        rpc_one(&conn, req),
    )
    .await
    {
        Ok(Ok(r)) => r,
        Ok(Err(_)) | Err(_) => return RpcResult::Failed,
    };

    match resp {
        DhtResponse::FindNode(r) => RpcResult::FindNodeReply(r.closer),
        DhtResponse::FindValue(r) => match r.result {
            WireFindValueOutcome::Found(rec) => RpcResult::FindValueFound(rec),
            WireFindValueOutcome::NotPresent => RpcResult::FindValueNotPresent,
            WireFindValueOutcome::Closer(c) => RpcResult::FindValueClose(c),
        },
        // Wrong response variant — peer is misbehaving. Treat as failure.
        _ => RpcResult::Failed,
    }
}

/// Merge a peer's reply descriptors into the candidate pool, dropping
/// duplicates (already in `candidates`). Each new entry gets its
/// distance computed once.
fn integrate_descriptors(
    target: &[u8; 32], candidates: &mut Vec<Candidate>, new: &[NodeDescriptor],
) {
    let known: HashSet<NodeId> = candidates.iter().map(|c| c.desc.id).collect();
    for desc in new {
        if known.contains(&desc.id) {
            continue;
        }
        let dist = distance(target, &desc.id);
        candidates.push(Candidate { desc: desc.clone(), distance: dist });
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::AtomicU64;
    use std::sync::atomic::Ordering as AtomicOrdering;

    use ed25519_dalek::SigningKey;

    use super::*;
    use crate::dht::Dht;
    use crate::dht::DhtConfig;
    use crate::dht::dht_cf_descriptors;

    fn fresh_signing_key() -> SigningKey {
        static SEQ: AtomicU64 = AtomicU64::new(1);
        let n = SEQ.fetch_add(1, AtomicOrdering::SeqCst);
        let mut seed = [0u8; 32];
        seed[..8].copy_from_slice(&n.to_le_bytes());
        seed[31] = (n & 0xff) as u8;
        SigningKey::from_bytes(&seed)
    }

    fn fresh_dht(self_id: NodeId) -> Arc<Dht> {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let id = SEQ.fetch_add(1, AtomicOrdering::SeqCst);
        let pid = std::process::id();
        let path = std::env::temp_dir().join(format!("promtuz-lookup-test-{pid}-{id}"));
        let _ = std::fs::remove_dir_all(&path);

        let mut opts = rust_rocksdb::Options::default();
        opts.create_if_missing(true);
        opts.create_missing_column_families(true);

        let mut cfs = vec![rust_rocksdb::ColumnFamilyDescriptor::new(
            "default",
            rust_rocksdb::Options::default(),
        )];
        cfs.extend(dht_cf_descriptors());

        let db = rust_rocksdb::DB::open_cf_descriptors(&opts, &path, cfs).expect("open db");
        let signing = fresh_signing_key();
        let cfg = DhtConfig::default();
        Arc::new(Dht::new(self_id, signing, cfg, Arc::new(db)).expect("dht"))
    }

    /// `lookup_node` against an empty routing table must return
    /// `LookupError::NoCandidates` immediately — the rest of the
    /// iterative algorithm cannot be exercised without spinning up
    /// real QUIC connections (out of scope for unit tests; phase 2
    /// integration tests cover it).
    #[tokio::test(flavor = "current_thread")]
    async fn lookup_node_with_empty_routing_table_returns_no_candidates() {
        let mut self_seed = [0u8; 32];
        self_seed[0] = 1;
        let self_id = NodeId::new(self_seed);
        let dht = fresh_dht(self_id);

        let mut target_seed = [0u8; 32];
        target_seed[0] = 2;
        let target = NodeId::new(target_seed);

        let result = lookup_node(dht, target).await;
        assert!(matches!(result, Err(LookupError::NoCandidates)));
    }

    /// `lookup_value` mirrors the same guard.
    #[tokio::test(flavor = "current_thread")]
    async fn lookup_value_with_empty_routing_table_returns_no_candidates() {
        let mut self_seed = [0u8; 32];
        self_seed[0] = 1;
        let self_id = NodeId::new(self_seed);
        let dht = fresh_dht(self_id);

        let user_ipk = [7u8; 32];
        let result = lookup_value(dht, user_ipk).await;
        assert!(matches!(result, Err(LookupError::NoCandidates)));
    }
}
