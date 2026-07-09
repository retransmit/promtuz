package com.promtuz.core

import com.promtuz.core.adapter.CoreEventBus
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.conflate
import kotlinx.coroutines.flow.filter
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.onStart

/**
 * Observe a DB-backed query: [read] once on subscribe (so a fresh collector sees
 * current truth), then re-read whenever the on_db_changed doorbell reports a table
 * this query touches. The doorbell is a bare signal — the truth comes from [read]
 * against the DB, never from an event payload (REACTIVE_UI.md). `conflate` collapses
 * a write burst into one re-read; the initial empty tick passes the table filter.
 */
fun <T> observeQuery(tables: Set<String>, read: suspend () -> T): Flow<T> =
    CoreEventBus.dbChanged
        .onStart { emit(emptySet()) }
        .filter { it.isEmpty() || it.any { table -> table in tables } }
        .conflate()
        .map { read() }
