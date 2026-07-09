package com.promtuz.chat.domain.model

/** Outgoing delivery state — mirrors libcore's status byte (0..4). Receipts only advance it. */
enum class SendStatus {
    Pending, Sent, Failed, Delivered, Read;

    companion object {
        fun from(raw: Int): SendStatus = entries.getOrElse(raw) { Pending }
    }
}
