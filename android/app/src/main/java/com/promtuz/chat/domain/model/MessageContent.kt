package com.promtuz.chat.domain.model

/**
 * A message's payload. libcore is text-only today; image/voice/file/sticker
 * variants (each carrying transfer state) land as the blob path ships — add them
 * as subtypes then and the bubble switches on the variant.
 */
sealed interface MessageContent {
    data class Text(val text: String) : MessageContent
}
