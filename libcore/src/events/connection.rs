use std::sync::atomic::Ordering;

use serde::Serialize;

use crate::state::CONNECTION_STATE;
use crate::events::Emittable;

#[derive(Serialize, Debug, Clone, PartialEq, Eq, uniffi::Enum)]
#[allow(unused)]
#[repr(i32)]
pub enum ConnectionState {
    Disconnected,
    Idle,
    Resolving,
    Connecting,
    Handshaking,
    Connected,
    Reconnecting,
    Failed,
    NoInternet,
    /// Link + auth are up, but we're still pulling the offline backlog
    /// (welcomes, deferred sends, queued messages) into the local DB.
    /// Appended last to keep every prior ordinal stable (the client maps by
    /// ordinal).
    Syncing,
}

impl Emittable for ConnectionState {
    fn emit(self) {
        CONNECTION_STATE.store(self.clone() as i32, Ordering::Relaxed);

        if let Some(events) = crate::platform::EVENTS.get() {
            events.on_connection(self);
        }
    }
}
