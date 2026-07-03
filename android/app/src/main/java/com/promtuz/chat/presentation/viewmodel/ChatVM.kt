package com.promtuz.chat.presentation.viewmodel

import android.app.Application
import android.content.Context
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.promtuz.chat.domain.model.UiMessage
import com.promtuz.chat.domain.model.UiMessagePosition
import com.promtuz.chat.domain.model.UiMessageStatus
import com.promtuz.core.CoreBridge
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import timber.log.Timber
import uniffi.core.MessageEvent
import uniffi.core.MessageRecord

class ChatVM(
    private val application: Application
) : ViewModel() {
    private val context: Context get() = application.applicationContext

    private val _messages = MutableStateFlow(emptyList<UiMessage>())
    val messages: StateFlow<List<UiMessage>> = _messages.asStateFlow()

    /** Set by the Chat activity before the screen renders */
    var peerIpk: ByteArray = ByteArray(32)
        private set

    private var initialized = false

    fun init(peerIpk: ByteArray) {
        // Guard re-entry: the activity re-calls this on recreation (rotation),
        // but the ViewModel + its viewModelScope survive — without this we'd
        // stack a second messageEvents collector each time.
        if (initialized) return
        initialized = true
        this.peerIpk = peerIpk
        loadMessages()
        listenForIncoming()
    }

    private fun loadMessages() {
        viewModelScope.launch {
            try {
                val rows = CoreBridge.messages(peerIpk, 50, "")
                _messages.value = rows.toUi()
            } catch (e: Exception) {
                Timber.tag("ChatVM").e(e, "Failed to load messages")
            }
        }
    }

    private fun listenForIncoming() {
        viewModelScope.launch {
            CoreBridge.messageEvents.collect { event ->
                when (event) {
                    is MessageEvent.Received -> {
                        if (event.from.contentEquals(peerIpk)) {
                            appendMessage(UiMessage(
                                event.id,
                                event.content,
                                false,
                                UiMessagePosition.Single,
                                event.timestamp.toLong() * 1000,
                                null
                            ))
                        }
                    }
                    is MessageEvent.Sent -> {
                        if (event.to.contentEquals(peerIpk)) {
                            appendMessage(UiMessage(
                                event.id,
                                event.content,
                                true,
                                UiMessagePosition.Single,
                                event.timestamp.toLong() * 1000,
                                UiMessageStatus.Sent
                            ))
                        }
                    }
                    is MessageEvent.Failed -> {
                        Timber.tag("ChatVM").w("Message failed: ${event.reason}")
                    }
                }
            }
        }
    }

    private fun appendMessage(msg: UiMessage) {
        _messages.update { current ->
            // Don't add duplicates (if it was already loaded from DB)
            if (current.any { it.id == msg.id }) return@update current
            recomputePositions(current + msg)
        }
    }

    fun dispatchMessage(content: String) {
        viewModelScope.launch {
            try {
                CoreBridge.sendMessage(peerIpk, content)
            } catch (e: Exception) {
                Timber.tag("ChatVM").e(e, "Failed to send message")
            }
        }
    }

    private fun List<MessageRecord>.toUi(): List<UiMessage> = mapIndexed { i, m ->
        val prev = getOrNull(i - 1)
        val next = getOrNull(i + 1)

        val samePrev = prev?.outgoing == m.outgoing
        val sameNext = next?.outgoing == m.outgoing

        val position = when {
            samePrev && sameNext -> UiMessagePosition.Middle
            samePrev && !sameNext -> UiMessagePosition.Start
            !samePrev && sameNext -> UiMessagePosition.End
            else -> UiMessagePosition.Single
        }

        UiMessage(
            m.id, m.content, m.outgoing, position, m.timestamp.toLong() * 1000, UiMessageStatus.entries[m.status.toInt()]
        )
    }

    private fun recomputePositions(list: List<UiMessage>): List<UiMessage> =
        list.mapIndexed { i, m ->
            val prev = list.getOrNull(i - 1)
            val next = list.getOrNull(i + 1)

            val samePrev = prev?.isSent == m.isSent
            val sameNext = next?.isSent == m.isSent

            val position = when {
                samePrev && sameNext -> UiMessagePosition.Middle
                samePrev && !sameNext -> UiMessagePosition.Start
                !samePrev && sameNext -> UiMessagePosition.End
                else -> UiMessagePosition.Single
            }

            UiMessage(m.id, m.content, m.isSent, position, m.timestamp, m.status)
        }
}
