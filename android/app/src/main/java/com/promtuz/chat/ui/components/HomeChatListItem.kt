package com.promtuz.chat.ui.components

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.defaultMinSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.ColorScheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.promtuz.chat.domain.model.ChatSummary
import com.promtuz.chat.domain.model.Presence
import com.promtuz.chat.utils.common.parseMessageDate

@Composable
fun HomeChatListItem(
    chat: ChatSummary,
    presence: Presence?,
    typing: Boolean,
    modifier: Modifier = Modifier,
    onOpen: () -> Unit,
) {
    val type = MaterialTheme.typography
    val colors = MaterialTheme.colorScheme
    val unread = chat.unreadCount > 0

    Row(
        modifier
            .fillMaxWidth()
            .clickable(onClick = onOpen)
            .padding(horizontal = 16.dp, vertical = 9.dp),
        horizontalArrangement = Arrangement.spacedBy(12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Avatar(chat.name, statusColor = presenceColor(presence))

        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(3.dp)) {
            Row(
                Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    chat.name,
                    Modifier.weight(1f),
                    style = type.titleMediumEmphasized,
                    fontWeight = if (unread) FontWeight.Bold else null,
                    color = colors.onSurface,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                if (chat.timestampMs > 0) Row(
                    horizontalArrangement = Arrangement.spacedBy(3.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    if (chat.lastOutgoing && !typing) DeliveryTick(chat.lastStatus)
                    Text(
                        parseMessageDate(chat.timestampMs),
                        style = type.bodySmallEmphasized,
                        color = if (unread) colors.primary else colors.onSurfaceVariant.copy(0.7f),
                    )
                }
            }

            Row(
                Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                val (line, lineColor) = statusLine(chat, typing, colors)
                Text(
                    line,
                    Modifier.weight(1f),
                    style = type.bodySmallEmphasized,
                    color = lineColor,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                if (unread) UnreadBadge(chat.unreadCount, colors)
            }
        }
    }
}

/** The preview/status line: live typing beats pairing state beats last message. */
private fun statusLine(chat: ChatSummary, typing: Boolean, colors: ColorScheme): Pair<String, Color> = when {
    typing -> "typing…" to colors.primary
    chat.status == 0 -> "Waiting to connect…" to colors.primary.copy(0.8f)
    chat.status == 2 -> "Couldn't connect" to colors.error.copy(0.85f)
    chat.lastDeleted -> "deleted message" to colors.onSurfaceVariant.copy(0.6f)
    chat.lastPreview.isNullOrEmpty() -> "No messages yet" to colors.onSurfaceVariant.copy(0.6f)
    else -> {
        val text = if (chat.lastOutgoing) "You: ${chat.lastPreview}" else chat.lastPreview
        val col = if (chat.unreadCount > 0) colors.onSurface.copy(0.9f) else colors.onSurfaceVariant.copy(0.7f)
        text to col
    }
}

/** Delivery tick for our last message; nothing while still pending. */
@Composable
private fun DeliveryTick(status: Int) {
    val colors = MaterialTheme.colorScheme
    val (glyph, color) = when (status) {
        2 -> "!" to colors.error
        3 -> "✓✓" to colors.onSurfaceVariant.copy(0.7f)
        4 -> "✓✓" to colors.primary
        1 -> "✓" to colors.onSurfaceVariant.copy(0.7f)
        else -> return
    }
    Text(glyph, style = MaterialTheme.typography.labelMedium, color = color, maxLines = 1)
}

@Composable
private fun UnreadBadge(count: Int, colors: ColorScheme) {
    Box(
        Modifier
            .heightIn(min = 20.dp)
            .defaultMinSize(minWidth = 20.dp)
            .clip(CircleShape)
            .background(colors.primary)
            .padding(horizontal = 6.dp),
        contentAlignment = Alignment.Center,
    ) {
        Text(
            if (count > 99) "99+" else "$count",
            style = MaterialTheme.typography.labelMedium,
            fontWeight = FontWeight.Bold,
            color = colors.onPrimary,
            maxLines = 1,
        )
    }
}

private val OnlineDot = Color(0xFF34C759)
private val IdleDot = Color(0xFFF5A623)

/** Generic status colour for the avatar dot; null hides it (offline/unknown). */
private fun presenceColor(p: Presence?): Color? = when (p) {
    Presence.Online -> OnlineDot
    is Presence.Idle -> IdleDot
    else -> null
}
