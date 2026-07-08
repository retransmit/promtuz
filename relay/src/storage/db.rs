//! The relay's on-disk store: one fjall `Database`, several keyspaces.
//!
//! Keyspaces (fjall's column-family equivalent — each its own LSM-tree):
//! - `messages`       sender-relay local fallback queue (`MessageKey` -> DispatchP).
//! - `dht_queue`      home-replica offline queue (`MessageKey`, per-recipient prefix).
//! - `dht_keypackage` MLS KeyPackage stash (per-IPK prefix).
//! - `dht_welcome`    MLS Welcome stash (per-recipient prefix).
//!
//! fjall does exact prefix scans natively, so no prefix-extractor config is
//! needed (unlike RocksDB). Durability-critical writes go through
//! [`Store::put_sync`] (insert + fsync); everything else is journal-buffered.

use std::path::Path;

use anyhow::Context;
use anyhow::Result;
use fjall::Database;
use fjall::Keyspace;
use fjall::KeyspaceCreateOptions;
use fjall::PersistMode;
use fjall::UserKey;
use fjall::UserValue;

pub const KS_MESSAGES: &str = "messages";
pub const KS_DHT_QUEUE: &str = "dht_queue";
pub const KS_DHT_KEYPACKAGE: &str = "dht_keypackage";
pub const KS_DHT_WELCOME: &str = "dht_welcome";

/// Owns the relay's fjall `Database` and its keyspace handles. Shared as
/// `Arc<Store>` between the `Relay` (message queue) and the `Dht` (home
/// queue, MLS stashes) — both point at the same on-disk store.
pub struct Store {
    db:             Database,
    pub messages:   Keyspace,
    pub queue:      Keyspace,
    pub keypackage: Keyspace,
    pub welcome:    Keyspace,
}

impl std::fmt::Debug for Store {
    // fjall's `Database` / `Keyspace` handles aren't `Debug`; `Dht` and
    // `Relay` derive `Debug` and hold an `Arc<Store>`, so give them a stub.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Store").finish_non_exhaustive()
    }
}

impl Store {
    /// Open (creating if absent) the relay's fjall store at `path`.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let db = Database::builder(path).open().context("open fjall database")?;
        let messages =
            db.keyspace(KS_MESSAGES, KeyspaceCreateOptions::default).context("open `messages`")?;
        let queue =
            db.keyspace(KS_DHT_QUEUE, KeyspaceCreateOptions::default).context("open `dht_queue`")?;
        let keypackage = db
            .keyspace(KS_DHT_KEYPACKAGE, KeyspaceCreateOptions::default)
            .context("open `dht_keypackage`")?;
        let welcome = db
            .keyspace(KS_DHT_WELCOME, KeyspaceCreateOptions::default)
            .context("open `dht_welcome`")?;
        Ok(Self { db, messages, queue, keypackage, welcome })
    }

    /// Insert then fsync the journal — the durability contract the old
    /// `WriteOptions::set_sync(true)` writes relied on.
    pub fn put_sync(
        &self, ks: &Keyspace, key: impl Into<UserKey>, val: impl Into<UserValue>,
    ) -> fjall::Result<()> {
        ks.insert(key, val)?;
        self.db.persist(PersistMode::SyncAll)
    }

    /// A buffered, atomic multi-op batch (used for drain GC). Not fsynced — a
    /// crash re-delivers, and the client dedupes by id.
    pub fn batch(&self) -> fjall::OwnedWriteBatch {
        self.db.batch()
    }

    /// Delete every entry in all keyspaces and fsync. Live-safe: the relay owns
    /// the fjall writer, so no lock fight — the `pzrelay clear-db` reset path.
    /// Leaves the daemon's in-memory routing/connections intact.
    pub fn clear_all(&self) -> Result<usize> {
        let mut n = 0usize;
        for ks in [&self.messages, &self.queue, &self.keypackage, &self.welcome] {
            let keys: Vec<UserKey> = ks
                .iter()
                .map(|g| g.into_inner().map(|(k, _)| k))
                .collect::<fjall::Result<_>>()
                .context("iterate keyspace")?;
            for k in keys {
                ks.remove(k).context("remove key")?;
                n += 1;
            }
        }
        self.db.persist(PersistMode::SyncAll).context("persist after clear")?;
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicU64;
    use std::sync::atomic::Ordering;

    use super::*;

    fn fresh_store() -> Store {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let id = SEQ.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!("pz-cleardb-test-{}-{id}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        Store::open(&path).expect("open store")
    }

    #[test]
    fn clear_all_empties_every_keyspace() {
        let store = fresh_store();
        store.messages.insert("a".as_bytes(), "1".as_bytes()).unwrap();
        store.queue.insert("b".as_bytes(), "2".as_bytes()).unwrap();
        store.keypackage.insert("c".as_bytes(), "3".as_bytes()).unwrap();
        store.welcome.insert("d".as_bytes(), "4".as_bytes()).unwrap();

        let n = store.clear_all().expect("clear");
        assert_eq!(n, 4, "must report every deleted entry");
        for ks in [&store.messages, &store.queue, &store.keypackage, &store.welcome] {
            assert_eq!(ks.iter().count(), 0, "keyspace must be empty after clear");
        }
    }
}
