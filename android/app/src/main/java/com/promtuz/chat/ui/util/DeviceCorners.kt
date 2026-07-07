package com.promtuz.chat.ui.util

import android.os.Build
import android.view.RoundedCorner
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp

/**
 * The physical top-left screen corner radius, so a scaled-down card matches the device's own
 * curvature (what the predictive-back card wants). Falls back to [fallback] pre-S or when the
 * window hasn't reported its insets yet.
 */
@Composable
fun deviceCornerRadius(fallback: Dp = 20.dp): Dp {
    val view = LocalView.current
    val density = LocalDensity.current
    return remember(view) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.S) return@remember fallback
        val radiusPx = view.rootWindowInsets
            ?.getRoundedCorner(RoundedCorner.POSITION_TOP_LEFT)
            ?.radius
            ?: return@remember fallback
        if (radiusPx <= 0) fallback else with(density) { radiusPx.toDp() }
    }
}
