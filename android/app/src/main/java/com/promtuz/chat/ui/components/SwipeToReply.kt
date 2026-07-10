package com.promtuz.chat.ui.components

import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.Spring
import androidx.compose.animation.core.spring
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import com.promtuz.chat.R
import com.promtuz.chat.ui.appearance.LocalChatColors
import kotlin.math.roundToInt
import kotlinx.coroutines.launch

/**
 * Drag a message left to stage a reply: hard linear clamp at 80dp (no banding),
 * commit at 50dp on release, exactly one haptic at the threshold (guard resets when
 * dragged back under). The cue arrow behind the row fades/scales with progress.
 */
@Composable
fun SwipeToReply(
    enabled: Boolean,
    onReply: () -> Unit,
    modifier: Modifier = Modifier,
    content: @Composable () -> Unit,
) {
    val offsetX = remember { Animatable(0f) }
    val haptic = LocalHapticFeedback.current
    val scope = rememberCoroutineScope()
    val density = LocalDensity.current
    val clampPx = with(density) { 80.dp.toPx() }
    val commitPx = with(density) { 50.dp.toPx() }
    val accent = LocalChatColors.current.accent

    Box(modifier.fillMaxWidth()) {
        DrawableIcon(
            R.drawable.i_reply,
            Modifier
                .align(Alignment.CenterEnd)
                .padding(end = 18.dp)
                .size(22.dp)
                .graphicsLayer {
                    val p = (-offsetX.value / commitPx).coerceIn(0f, 1f)
                    alpha = p
                    scaleX = 0.6f + 0.4f * p
                    scaleY = 0.6f + 0.4f * p
                },
            tint = accent,
        )
        Box(
            Modifier
                .offset { IntOffset(offsetX.value.roundToInt(), 0) }
                .pointerInput(enabled) {
                    if (!enabled) return@pointerInput
                    var vibrated = false
                    detectHorizontalDragGestures(
                        onDragStart = { vibrated = false },
                        onDragEnd = {
                            if (offsetX.value <= -commitPx) onReply()
                            scope.launch {
                                offsetX.animateTo(0f, spring(stiffness = Spring.StiffnessMediumLow))
                            }
                        },
                        onDragCancel = { scope.launch { offsetX.animateTo(0f) } },
                    ) { change, dx ->
                        change.consume()
                        val next = (offsetX.value + dx).coerceIn(-clampPx, 0f)
                        scope.launch { offsetX.snapTo(next) }
                        if (next <= -commitPx) {
                            if (!vibrated) {
                                vibrated = true
                                haptic.performHapticFeedback(HapticFeedbackType.GestureThresholdActivate)
                            }
                        } else vibrated = false
                    }
                },
        ) { content() }
    }
}
