package com.promtuz.core.adapter

import com.promtuz.chat.presentation.state.ConnectionState
import kotlinx.coroutines.channels.BufferOverflow
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.asStateFlow
import uniffi.core.CoreEvents
import uniffi.core.MessageEvent
import uniffi.core.ConnectionState as FfiConnectionState

/**
 * The client's [CoreEvents] port. core calls these off-main (tokio threads);
 * we marshal them into flows and never touch UI directly. Bodies must not
 * throw — a throw across the FFI callback boundary would abort core — so both
 * paths are non-throwing (bounded, drop-oldest emit; ordinal lookup falls
 * back to Idle).
 */
object CoreEventBus : CoreEvents {
    private val _connection = MutableStateFlow(ConnectionState.Idle)
    val connection: StateFlow<ConnectionState> = _connection.asStateFlow()

    private val _messages = MutableSharedFlow<MessageEvent>(
        replay = 0,
        extraBufferCapacity = 64,
        onBufferOverflow = BufferOverflow.DROP_OLDEST,
    )
    val messages: SharedFlow<MessageEvent> = _messages.asSharedFlow()

    override fun onConnection(state: FfiConnectionState) {
        // Generated enum shares the app enum's order 1:1 (Disconnected..NoInternet).
        _connection.value = ConnectionState.entries.getOrElse(state.ordinal) { ConnectionState.Idle }
    }

    override fun onMessage(event: MessageEvent) {
        _messages.tryEmit(event)
    }
}
