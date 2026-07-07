package com.promtuz.chat.navigation

import androidx.activity.BackEventCompat
import androidx.activity.compose.PredictiveBackHandler
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.CubicBezierEasing
import androidx.compose.animation.core.tween
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.TransformOrigin
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.platform.LocalWindowInfo
import androidx.compose.ui.util.lerp
import androidx.lifecycle.viewmodel.navigation3.rememberViewModelStoreNavEntryDecorator
import androidx.navigation3.runtime.NavBackStack
import androidx.navigation3.runtime.NavEntry
import androidx.navigation3.runtime.NavKey
import androidx.navigation3.runtime.rememberDecoratedNavEntries
import androidx.navigation3.runtime.rememberSaveableStateHolderNavEntryDecorator
import com.promtuz.chat.ui.util.deviceCornerRadius
import kotlinx.coroutines.flow.Flow

/**
 * True for the card that is currently being animated off (predictive back or a pop). Read by
 * [com.promtuz.chat.ui.util.freezeOnExit] to freeze live blur before the scale hits it.
 */
val LocalNavCardExiting = compositionLocalOf { false }

private const val FWD_DUR = 260
private const val COMMIT_DUR = 340
private const val CANCEL_DUR = 260
private const val SCALE_TO = 0.90f
private val fwdEase = CubicBezierEasing(0.2f, 0.8f, 0.2f, 1f)
private val scaleEase = CubicBezierEasing(0.12f, 0.85f, 0.25f, 1f) // front-loaded: scale settles early in the drag
private val commitEase = CubicBezierEasing(0.3f, 0f, 0.1f, 1f)     // EASE_OUT_QUINT-ish fling

/**
 * A single-stack navigation display, ours end-to-end so we own every pixel of motion — built on
 * nav3's entry primitives ([rememberDecoratedNavEntries] keeps per-entry ViewModelStore + saved
 * state, so a revealed screen keeps its scroll and half-typed text). Replaces nav3's `NavDisplay`.
 *
 * Forward push slides the new screen in from the right over a parallaxing previous. Back is the
 * Telegram scale-card: the current screen scales toward the finger (drag), then slides off to the
 * right on release (commit) or springs back (cancel) — separate animations, which nav3's one-spec
 * seek could never split. Both gesture and back-button route through [PredictiveBackHandler], so the
 * pop is always animated *before* the entry leaves the stack — we never render a popped entry.
 */
@Composable
fun NavStage(
    backStack: NavBackStack<NavKey>,
    onBack: () -> Unit,
    modifier: Modifier = Modifier,
    entryProvider: (NavKey) -> NavEntry<NavKey>,
) {
    val entries = rememberDecoratedNavEntries(
        backStack,
        listOf(
            rememberSaveableStateHolderNavEntryDecorator(),
            rememberViewModelStoreNavEntryDecorator(),
        ),
        entryProvider,
    )
    val top = entries.last()
    val below = entries.getOrNull(entries.lastIndex - 1)

    val cardShape = RoundedCornerShape(deviceCornerRadius())
    val size = LocalWindowInfo.current.containerSize
    val widthPx = size.width.toFloat().coerceAtLeast(1f)
    val heightPx = size.height.toFloat().coerceAtLeast(1f)

    // Forward (push): new top slides in from the right, the screen we came from parallaxes left.
    // `forward` is derived in composition (not an effect) so the new screen is already offscreen on
    // the very first frame of the push — no one-frame flash of it rendered at rest.
    val topKey = top.contentKey
    var shownKey by remember { mutableStateOf(topKey) }
    var shownSize by remember { mutableStateOf(backStack.size) }
    val forward = topKey != shownKey && backStack.size > shownSize
    val enter = remember { Animatable(1f) } // 0 = new fully offscreen right, 1 = settled
    var pushing by remember { mutableStateOf(false) }
    LaunchedEffect(topKey) {
        val isForward = topKey != shownKey && backStack.size > shownSize
        shownKey = topKey
        shownSize = backStack.size
        if (isForward) {
            pushing = true
            enter.snapTo(0f)
            enter.animateTo(1f, tween(FWD_DUR, easing = fwdEase))
            pushing = false
        }
    }
    // enter.value is read ONLY inside the graphicsLayer lambdas below, never in composition — so the
    // push animates by redraw, with no per-frame recomposition of the stage or the screens.
    val showPush = forward || pushing

    // Back / predictive: gesture-driven scale, then a separate commit slide-off or cancel spring.
    var backActive by remember { mutableStateOf(false) }
    var touchY by remember { mutableStateOf(heightPx / 2f) }
    var fromRight by remember { mutableStateOf(false) }
    val progress = remember { Animatable(0f) } // scale progress, follows the finger
    val commit = remember { Animatable(0f) }   // 0 → 1 slides the card off to the right

    PredictiveBackHandler(enabled = entries.size > 1) { events: Flow<BackEventCompat> ->
        backActive = true
        try {
            events.collect { e ->
                touchY = e.touchY
                fromRight = e.swipeEdge == BackEventCompat.EDGE_RIGHT
                progress.snapTo(e.progress)
            }
            // Released past threshold → complete: fling the (still-scaled) card off, then pop.
            commit.animateTo(1f, tween(COMMIT_DUR, easing = commitEase))
            onBack()
            commit.snapTo(0f)
            progress.snapTo(0f)
            backActive = false
        } catch (c: Throwable) {
            // Cancelled → spring the card back to rest.
            progress.animateTo(0f, tween(CANCEL_DUR, easing = fwdEase))
            backActive = false
            throw c
        }
    }

    Box(modifier.fillMaxSize()) {
        // The revealed screen sits behind: the pop target during a back gesture, or the origin
        // screen parallaxing left as the new one covers it during a forward push.
        if (backActive || showPush) {
            below?.let { b ->
                val behind = if (showPush && !backActive) {
                    Modifier.graphicsLayer { translationX = -(if (forward) 0f else enter.value) * widthPx * 0.25f }
                } else Modifier
                Card(b, behind, cardShape, exiting = false)
            }
        }

        // The front card.
        val front = when {
            backActive -> Modifier.graphicsLayer {
                val s = lerp(1f, SCALE_TO, scaleEase.transform(progress.value))
                scaleX = s
                scaleY = s
                transformOrigin = TransformOrigin(
                    if (fromRight) 0.12f else 0.88f,
                    (touchY / heightPx).coerceIn(0f, 1f),
                )
                translationX = commit.value * this.size.width
            }
            showPush -> Modifier.graphicsLayer { translationX = (1f - (if (forward) 0f else enter.value)) * widthPx }
            else -> Modifier
        }
        Card(top, front, cardShape, exiting = backActive)
    }
}

@Composable
private fun Card(entry: NavEntry<NavKey>, transform: Modifier, shape: androidx.compose.ui.graphics.Shape, exiting: Boolean) {
    key(entry.contentKey) {
        Box(Modifier.fillMaxSize().then(transform).clip(shape)) {
            CompositionLocalProvider(LocalNavCardExiting provides exiting) {
                entry.Content()
            }
        }
    }
}
