package com.promtuz.chat.ui.appearance

import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.ProvidableCompositionLocal
import androidx.compose.runtime.ReadOnlyComposable
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp

/**
 * The active chat look, read by every chat renderer via [LocalChatAppearance].
 * Static [ChatAppearance.Default] today; a persisted store feeds it later — that
 * swap touches only [ChatAppearanceProvider], nothing downstream.
 */
val LocalChatAppearance: ProvidableCompositionLocal<ChatAppearance> =
    staticCompositionLocalOf { ChatAppearance.Default }

@Composable
fun ChatAppearanceProvider(
    appearance: ChatAppearance = ChatAppearance.Default,
    content: @Composable () -> Unit,
) {
    CompositionLocalProvider(LocalChatAppearance provides appearance, content = content)
}

// ── token → Compose conversions (the primitive-to-Compose boundary) ────────────

val BubbleStyle.corner: Dp get() = cornerRadius.dp
val BubbleStyle.nearCorner: Dp get() = nearCornerRadius.dp
val LayoutStyle.messageGapDp: Dp get() = messageGap.dp
val LayoutStyle.groupGapDp: Dp get() = groupGap.dp

/** A `null` color token means "derive from the M3 role" — resolved here so light/dark just works. */
private fun Long?.orRole(role: Color): Color = this?.let { Color(it) } ?: role

@Composable @ReadOnlyComposable
fun BubbleColors.outgoingBubble(): Color = outgoing.orRole(MaterialTheme.colorScheme.primaryContainer)

@Composable @ReadOnlyComposable
fun BubbleColors.incomingBubble(): Color = incoming.orRole(MaterialTheme.colorScheme.surfaceContainerHigh)

@Composable @ReadOnlyComposable
fun BubbleColors.outgoingContent(): Color = outgoingText.orRole(MaterialTheme.colorScheme.onPrimaryContainer)

@Composable @ReadOnlyComposable
fun BubbleColors.incomingContent(): Color = incomingText.orRole(MaterialTheme.colorScheme.onSurface)
