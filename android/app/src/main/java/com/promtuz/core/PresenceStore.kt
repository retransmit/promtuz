package com.promtuz.core

import android.content.Context
import android.content.SharedPreferences
import androidx.core.content.edit
import com.promtuz.chat.domain.model.Presence
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json

@Serializable
private data class PresenceEntry(val kind: Int, val ts: Long)

@Serializable
private data class PresenceSnapshot(val savedAt: Long, val peers: Map<String, PresenceEntry>)

/**
 * Persists the last-known presence per contact (hex IPK) so a cold app start
 * shows last-seens immediately instead of a blank — the relay can't re-serve an
 * offline contact's last-seen (their subscription is dropped on disconnect, so
 * the mutual-consent snapshot excludes them). Live states (online/idle) can't be
 * trusted across a restart, so [seed] downgrades them to "last seen" until a
 * fresh delta reconfirms. One JSON blob in prefs; written off the hot path.
 */
object PresenceStore {
    private const val KEY = "presence"
    private lateinit var prefs: SharedPreferences
    private val json = Json { ignoreUnknownKeys = true }

    fun init(context: Context) {
        prefs = context.getSharedPreferences("presence", Context.MODE_PRIVATE)
    }

    /** Cold-start seed: persisted states, with live ones downgraded to last-seen. */
    fun seed(): Map<String, Presence> {
        val snap = prefs.getString(KEY, null)
            ?.let { runCatching { json.decodeFromString<PresenceSnapshot>(it) }.getOrNull() }
            ?: return emptyMap()
        return snap.peers.mapValues { (_, e) -> restore(e, snap.savedAt) }
    }

    fun save(map: Map<String, Presence>, savedAt: Long) {
        val peers = map.mapValues { (_, p) -> entry(p) }
        prefs.edit { putString(KEY, json.encodeToString(PresenceSnapshot.serializer(), PresenceSnapshot(savedAt, peers))) }
    }

    private fun entry(p: Presence): PresenceEntry = when (p) {
        Presence.Online -> PresenceEntry(0, 0)
        is Presence.Idle -> PresenceEntry(1, p.sinceMs)
        is Presence.LastSeen -> PresenceEntry(2, p.atMs)
        Presence.Unknown -> PresenceEntry(3, 0)
    }

    // On load we can't confirm liveness: a persisted "online" becomes "last seen
    // when we last had contact", and "idle since T" becomes "last seen T" (their
    // last active moment). A fresh subscribe snapshot upgrades still-online peers.
    private fun restore(e: PresenceEntry, savedAt: Long): Presence = when (e.kind) {
        0 -> Presence.LastSeen(savedAt)
        1 -> Presence.LastSeen(e.ts)
        2 -> Presence.LastSeen(e.ts)
        else -> Presence.Unknown
    }
}
