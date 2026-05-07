use common::proto::client_rel::DeliverP;
use common::proto::pack::Unpacker;
use common::quic::id::UserId;
use rust_rocksdb::ColumnFamilyDescriptor;
use rust_rocksdb::DB;
use rust_rocksdb::Options;
use rust_rocksdb::SliceTransform;

#[path = "../storage/mod.rs"]
mod storage;

use storage::MessageKey;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut opts = Options::default();
    opts.set_prefix_extractor(SliceTransform::create_fixed_prefix(32));

    // The relay's DB now declares DHT column families (see
    // `relay/src/util/rocksdb.rs`). Listing them all and including
    // their names here keeps `ldb` compatible whether or not the DHT
    // CFs are present — `list_cf` returns whatever the on-disk file
    // actually has.
    let cfs = match DB::list_cf(&Options::default(), "./db") {
        Ok(names) => names,
        // Empty / freshly-created DB → just the default CF.
        Err(_) => vec!["default".into()],
    };
    let cf_descriptors: Vec<_> = cfs
        .iter()
        .map(|name| {
            let mut cf_opts = Options::default();
            if name == "default" {
                cf_opts.set_prefix_extractor(SliceTransform::create_fixed_prefix(32));
            }
            ColumnFamilyDescriptor::new(name, cf_opts)
        })
        .collect();

    let db = DB::open_cf_descriptors(&opts, "./db", cf_descriptors)?;

    let mut iter = db.iterator(rust_rocksdb::IteratorMode::Start);

    while let Some(Ok((key, value))) = iter.next() {
        let Some(key) = MessageKey::parse(&key[..]) else {
            eprintln!("invalid key length: {}", key.len());
            continue;
        };
        let time = u64::from_be_bytes(key.ts_be);

        let msg = DeliverP::deser(&value[..]).map_err(|_| value);

        println!(
            "{ipk} | {time} | {id} - {msg:?}",
            ipk = UserId::derive(&key.recipient),
            id = hex::encode(key.id)
        );
    }

    Ok(())
}
