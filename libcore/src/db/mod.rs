use std::{fs, path::Path, process};

mod macros;

pub mod identity;
pub mod messages;
pub mod mls;
pub mod network;
pub mod outbox;
pub mod peers;
pub mod utils;

static PACKAGE_NAME: &str = "com.promtuz.chat";

pub fn db(file_name: &'static str) -> String {
    // On-device this is the fixed Android package data dir. Off-device
    // (the e2e sandbox, or any host run) `PROMTUZ_DATA_DIR` redirects every
    // libcore store into an isolated temp dir, so N client processes don't
    // collide on the one global path. Unset → exact on-device behaviour.
    let db_dir = std::env::var("PROMTUZ_DATA_DIR")
        .unwrap_or_else(|_| format!("/data/data/{PACKAGE_NAME}/databases"));
    let dir_path = Path::new(&db_dir);

    // `create_dir_all` (was `create_dir`): an override dir may be nested
    // (e.g. `/tmp/promtuz-e2e/client-0/databases`) with missing parents.
    if !dir_path.is_dir() && fs::create_dir_all(dir_path).is_err() {
        log::error!("Failed to create database directory!");
        process::exit(1);
    }

    format!("{db_dir}/{file_name}.db")
}
