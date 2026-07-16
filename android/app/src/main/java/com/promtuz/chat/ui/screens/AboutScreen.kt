package com.promtuz.chat.ui.screens

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import com.promtuz.chat.BuildConfig
import com.promtuz.chat.presentation.viewmodel.UpdateVM
import com.promtuz.chat.ui.components.FlexibleScreen
import com.promtuz.chat.ui.components.UpdateSheet
import org.koin.androidx.compose.koinViewModel

@Composable
fun AboutScreen(updates: UpdateVM = koinViewModel()) {
    val context = LocalContext.current
    val packageInfo = remember { context.packageManager.getPackageInfo(context.packageName, 0) }
    val channel = if (BuildConfig.DEBUG) "Debug" else "Release"
    var showSheet by remember { mutableStateOf(false) }

    FlexibleScreen({ Text("About Promtuz") }) { padding, _ ->
        Column(
            Modifier.fillMaxSize().padding(padding).padding(horizontal = 20.dp, vertical = 18.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            Text(
                "${packageInfo.versionName}  ·  $channel channel",
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                style = MaterialTheme.typography.bodyLarge,
            )
            OutlinedButton(onClick = { updates.check(); showSheet = true }) { Text("Check for updates") }
        }
    }

    if (showSheet) UpdateSheet(onDismiss = { showSheet = false })
}
