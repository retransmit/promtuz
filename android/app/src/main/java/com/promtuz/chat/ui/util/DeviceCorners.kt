package com.promtuz.chat.ui.util

import android.view.RoundedCorner
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp

/**
 * The physical top-left screen corner radius — so a card can match the device's own curvature.
 * Returns [fallback] (0 = square) when the screen has flat corners or hasn't reported insets yet;
 * a flat-corner device should read as 0 at rest, and get rounded only mid-gesture.
 */
@Composable
fun deviceCornerRadius(fallback: Dp = 0.dp): Dp {
    val view = LocalView.current
    val density = LocalDensity.current
    return remember(view) {
        val radiusPx = view.rootWindowInsets
            ?.getRoundedCorner(RoundedCorner.POSITION_TOP_LEFT)
            ?.radius
            ?: return@remember fallback
        if (radiusPx <= 0) fallback else with(density) { radiusPx.toDp() }
    }
}
