package com.promtuz.chat.ui.screens

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import com.promtuz.chat.BuildConfig
import com.promtuz.chat.presentation.viewmodel.UpdateVM
import com.promtuz.chat.ui.components.FlexibleScreen
import com.promtuz.chat.update.UpdateManifest
import com.promtuz.chat.update.UpdateState
import org.koin.androidx.compose.koinViewModel
import java.util.Locale

@Composable
fun AboutScreen(viewModel: UpdateVM = koinViewModel()) {
    val context = LocalContext.current
    val state by viewModel.state.collectAsState()
    val packageInfo = remember { context.packageManager.getPackageInfo(context.packageName, 0) }
    val channel = if (BuildConfig.DEBUG) "Debug" else "Release"
    var dismissedVersion by remember { mutableStateOf<Int?>(null) }

    FlexibleScreen({ Text("About") }) { padding, _ ->
        Column(
            Modifier.fillMaxSize().padding(padding).padding(horizontal = 20.dp, vertical = 18.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            Text("Promtuz", style = MaterialTheme.typography.headlineMedium)
            Text(
                "${packageInfo.versionName}  ·  $channel channel",
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                style = MaterialTheme.typography.bodyLarge,
            )
            UpdateCard(
                state = state,
                onCheck = viewModel::check,
                onDownload = viewModel::download,
                onCancelDownload = viewModel::cancelDownload,
                onInstall = viewModel::install,
                onPermission = viewModel::requestInstallPermission,
            )
        }
    }

    val available = state as? UpdateState.Available
    if (available != null && dismissedVersion != available.manifest.versionCode) {
        UpdatePrompt(
            manifest = available.manifest,
            action = "Download update",
            onDismiss = { dismissedVersion = available.manifest.versionCode },
            onAction = { viewModel.download(available.manifest) },
        )
    }
    val ready = state as? UpdateState.Ready
    if (ready != null && dismissedVersion != ready.manifest.versionCode) {
        UpdatePrompt(
            manifest = ready.manifest,
            action = "Install update",
            onDismiss = { dismissedVersion = ready.manifest.versionCode },
            onAction = { viewModel.install(ready.manifest, ready.apk) },
        )
    }
}

@Composable
private fun UpdateCard(
    state: UpdateState,
    onCheck: () -> Unit,
    onDownload: (UpdateManifest) -> Unit,
    onCancelDownload: () -> Unit,
    onInstall: (UpdateManifest, java.io.File) -> Unit,
    onPermission: () -> Unit,
) {
    Card(Modifier.fillMaxWidth()) {
        Column(Modifier.padding(20.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            Text("App updates", style = MaterialTheme.typography.titleLarge)
            when (state) {
                UpdateState.None -> {
                    Text("You are up to date.", color = MaterialTheme.colorScheme.onSurfaceVariant)
                    OutlinedButton(onClick = onCheck) { Text("Check for updates") }
                }
                UpdateState.Checking -> Row(verticalAlignment = Alignment.CenterVertically) {
                    CircularProgressIndicator(Modifier.width(20.dp), strokeWidth = 2.dp)
                    Spacer(Modifier.width(12.dp))
                    Text("Checking for a signed update…")
                }
                is UpdateState.Available -> {
                    Text("Version ${state.manifest.versionName} is ready to download.")
                    Button(onClick = { onDownload(state.manifest) }) { Text("Download update") }
                }
                is UpdateState.Downloading -> {
                    Text("Downloading ${state.manifest.versionName}")
                    LinearProgressIndicator({ state.progress }, Modifier.fillMaxWidth())
                    Text("${(state.progress * 100).toInt()}%", color = MaterialTheme.colorScheme.onSurfaceVariant)
                    OutlinedButton(onClick = onCancelDownload) { Text("Cancel download") }
                }
                is UpdateState.Ready -> {
                    Text("Version ${state.manifest.versionName} is verified and ready.")
                    Button(onClick = { onInstall(state.manifest, state.apk) }) { Text("Install update") }
                }
                is UpdateState.PermissionNeeded -> {
                    Text("Allow Promtuz to install this verified update.")
                    Button(onClick = onPermission) { Text("Allow installs") }
                    OutlinedButton(onClick = { onInstall(state.manifest, state.apk) }) { Text("Install update") }
                }
                is UpdateState.Error -> {
                    Text(state.message, color = MaterialTheme.colorScheme.error)
                    OutlinedButton(onClick = onCheck) { Text("Try again") }
                }
            }
        }
    }
}

@Composable
private fun UpdatePrompt(manifest: UpdateManifest, action: String, onDismiss: () -> Unit, onAction: () -> Unit) {
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Promtuz ${manifest.versionName}") },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Text("A signed update is available for this device.")
                Text(
                    "${formatSize(manifest.size)}  ·  Published ${manifest.publishedAt}",
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    style = MaterialTheme.typography.bodyMedium,
                )
            }
        },
        confirmButton = { Button(onClick = onAction) { Text(action) } },
        dismissButton = { OutlinedButton(onClick = onDismiss) { Text("Not now") } },
    )
}

private fun formatSize(bytes: Long): String = when {
    bytes >= 1_000_000 -> String.format(Locale.ROOT, "%.1f MB", bytes / 1_000_000.0)
    bytes >= 1_000 -> String.format(Locale.ROOT, "%.0f KB", bytes / 1_000.0)
    else -> "$bytes B"
}
