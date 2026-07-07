package com.promtuz.chat.ui.util

import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.graphics.layer.drawLayer
import androidx.compose.ui.graphics.rememberGraphicsLayer
import com.promtuz.chat.navigation.LocalNavCardExiting

/**
 * Bakes this node's rendered output — backdrop blur included — into a snapshot while its card is
 * animating out, so the predictive-back scale-card doesn't shatter live Haze blur.
 *
 * Haze samples the content behind it in screen space; an ancestor graphicsLayer *scale* samples
 * across that boundary and misaligns (the library author confirms it's unfixable — the effect can't
 * know the ancestor transform). The dodge: while [LocalNavCardExiting] is true we stop re-recording
 * and replay the last live frame — captured at scale ≈ 1.0, then scaled as flat pixels by the
 * ancestor. Freeze and thaw both land at scale 1.0, so the swap is seamless.
 */
@Composable
fun Modifier.freezeOnExit(): Modifier {
    val frozen = LocalNavCardExiting.current
    val layer = rememberGraphicsLayer()
    return drawWithContent {
        if (!frozen) layer.record { this@drawWithContent.drawContent() }
        drawLayer(layer)
    }
}
