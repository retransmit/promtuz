package com.promtuz.chat.ui.constants

import androidx.compose.animation.ContentTransform
import androidx.compose.animation.core.EaseInOutCirc
import androidx.compose.animation.core.TweenSpec
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.slideOutVertically
import androidx.compose.animation.togetherWith

object Tweens {
    fun <T> microInteraction(dur: Int = 150): TweenSpec<T> {
        return tween(dur, easing = EaseInOutCirc)
    }
}

object Buttonimations {
    fun labelSlide(): ContentTransform {
        return (slideInVertically(
            initialOffsetY = { fullHeight -> fullHeight }, animationSpec = Tweens.microInteraction()
        ) + fadeIn(Tweens.microInteraction())) togetherWith (slideOutVertically(
            targetOffsetY = { fullHeight -> -fullHeight }, animationSpec = Tweens.microInteraction()
        ) + fadeOut(Tweens.microInteraction()))
    }
}
