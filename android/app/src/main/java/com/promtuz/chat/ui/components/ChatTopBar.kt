package com.promtuz.chat.ui.components

import androidx.activity.compose.LocalOnBackPressedDispatcherOwner
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp
import com.promtuz.chat.R
import com.promtuz.chat.presentation.viewmodel.ChatVM
import dev.chrisbanes.haze.HazeState
import dev.chrisbanes.haze.HazeStyle
import dev.chrisbanes.haze.HazeTint
import dev.chrisbanes.haze.hazeEffect

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ChatTopBar(name: String, viewModel: ChatVM, haze: HazeState) {
    val back = LocalOnBackPressedDispatcherOwner.current
    val colors = MaterialTheme.colorScheme
    val typing by viewModel.typing.collectAsState()
    val hazeStyle = HazeStyle(colors.surface, HazeTint(colors.surface.copy(alpha = 0.5f)), 30.dp, 0f)

    TopAppBar(
        title = {
            Row(
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(10.dp),
            ) {
                Avatar(name, 40.dp)
                Column {
                    Text(name, style = MaterialTheme.typography.titleMediumEmphasized, maxLines = 1)
                    if (typing) Text(
                        "typing…",
                        style = MaterialTheme.typography.labelMedium,
                        color = colors.primary,
                    )
                }
            }
        },
        navigationIcon = {
            IconButton(onClick = { back?.onBackPressedDispatcher?.onBackPressed() }) {
                DrawableIcon(R.drawable.i_back_chevron)
            }
        },
        modifier = Modifier.hazeEffect(haze, hazeStyle),
        colors = TopAppBarDefaults.topAppBarColors(containerColor = Color.Transparent),
    )
}
