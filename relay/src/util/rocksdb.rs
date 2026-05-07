use anyhow::Result;
use rust_rocksdb::ColumnFamilyDescriptor;
use rust_rocksdb::DB;
use rust_rocksdb::Options;
use rust_rocksdb::SliceTransform;

use crate::dht::dht_cf_descriptors;

// static ROCKS_DB_FILE: &str = "relay.db";

/// Open the relay's RocksDB instance.
///
/// One DB, multiple column families:
/// - `default`: per-recipient message queue (existing). 32-byte fixed
///   prefix extractor for the `MessageKey::recipient` field
///   (`relay/src/storage/mod.rs`).
/// - `dht_presence` / `dht_merkle`: DHT replica state (`relay/src/dht`,
///   §1.2 of the DHT design doc). No prefix extractors — point lookups
///   on 32-byte keys (presence) and 3-byte keys (merkle).
///
/// `create_missing_column_families(true)` makes this idempotent: a
/// pre-DHT DB on disk gets its CFs created on first restart with the
/// DHT-aware binary. The opposite (DHT binary against a DB that already
/// has the CFs) "just works" — the descriptors match the on-disk
/// layout.
pub fn rocksdb() -> Result<DB> {
    let mut opts = Options::default();

    opts.create_if_missing(true);
    opts.create_missing_column_families(true);

    // Default-CF options. The 32-byte fixed prefix extractor groups all
    // messages for a single recipient (per `MessageKey` layout). It
    // belongs ONLY on the default CF — the DHT CFs use unrelated key
    // shapes (§1.2).
    let mut default_opts = Options::default();
    default_opts.set_prefix_extractor(SliceTransform::create_fixed_prefix(32));

    let mut cfs: Vec<ColumnFamilyDescriptor> =
        vec![ColumnFamilyDescriptor::new("default", default_opts)];
    cfs.extend(dht_cf_descriptors());

    let db = DB::open_cf_descriptors(&opts, "db", cfs)?;

    Ok(db)
}
