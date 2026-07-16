package com.promtuz.chat.ui.components

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LifecycleEventEffect
import com.promtuz.chat.presentation.viewmodel.UpdateVM
import com.promtuz.chat.update.UpdateManifest
import com.promtuz.chat.update.UpdateState
import org.koin.androidx.compose.koinViewModel
import java.util.Locale

/**
 * Update flow as a bottom sheet — the single surface for the whole UpdateState
 * machine, so a user can update from anywhere (top-bar icon, About) instead of
 * navigating to a dedicated page.
 */
@Composable
fun UpdateSheet(onDismiss: () -> Unit, viewModel: UpdateVM = koinViewModel()) {
    val state by viewModel.state.collectAsState()
    val context = LocalContext.current

    // Returning from the "install unknown apps" settings grant: the reason they
    // left was to install, so launch the installer straight away once it's ours.
    LifecycleEventEffect(Lifecycle.Event.ON_RESUME) {
        val s = state
        if (s is UpdateState.PermissionNeeded && context.packageManager.canRequestPackageInstalls()) {
            viewModel.install(s.manifest, s.apk)
        }
    }

    ModalBottomSheet(onDismissRequest = onDismiss) {
        Column(
            Modifier.fillMaxWidth().padding(horizontal = 24.dp).padding(bottom = 32.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            when (val s = state) {
                UpdateState.None -> {
                    Title("You're up to date")
                    OutlinedButton(viewModel::check, Modifier.fillMaxWidth()) { Text("Check again") }
                }
                UpdateState.Checking -> {
                    CircularProgressIndicator()
                    Subtitle("Checking for a signed update…")
                }
                is UpdateState.Available -> {
                    Title("Update available")
                    Subtitle(versionLine(s.manifest))
                    Button({ viewModel.download(s.manifest) }, Modifier.fillMaxWidth()) { Text("Download update") }
                }
                is UpdateState.Downloading -> {
                    Title("Downloading ${s.manifest.versionName}")
                    LinearProgressIndicator({ s.progress }, Modifier.fillMaxWidth())
                    Subtitle("${(s.progress * 100).toInt()}%")
                    OutlinedButton(viewModel::cancelDownload, Modifier.fillMaxWidth()) { Text("Cancel") }
                }
                is UpdateState.Ready -> {
                    Title("Ready to install")
                    Subtitle(versionLine(s.manifest))
                    Button({ viewModel.install(s.manifest, s.apk) }, Modifier.fillMaxWidth()) { Text("Install update") }
                }
                is UpdateState.PermissionNeeded -> {
                    Title("Allow installs")
                    Subtitle("Promtuz needs permission to install this verified update.")
                    Button(viewModel::requestInstallPermission, Modifier.fillMaxWidth()) { Text("Allow installs") }
                }
                is UpdateState.Error -> {
                    Title("Update failed")
                    Text(
                        s.message,
                        color = MaterialTheme.colorScheme.error,
                        style = MaterialTheme.typography.bodyMedium,
                        textAlign = TextAlign.Center,
                    )
                    OutlinedButton(viewModel::check, Modifier.fillMaxWidth()) { Text("Try again") }
                }
            }
        }
    }
}

private fun versionLine(m: UpdateManifest) = "${m.versionName}  ·  ${formatSize(m.size)}"

private fun formatSize(bytes: Long): String = when {
    bytes >= 1_000_000 -> String.format(Locale.ROOT, "%.1f MB", bytes / 1_000_000.0)
    bytes >= 1_000 -> String.format(Locale.ROOT, "%.0f KB", bytes / 1_000.0)
    else -> "$bytes B"
}

@Composable
private fun Title(text: String) = Text(
    text,
    style = MaterialTheme.typography.titleMedium,
    textAlign = TextAlign.Center,
)

@Composable
private fun Subtitle(text: String) = Text(
    text,
    style = MaterialTheme.typography.bodyMedium,
    color = MaterialTheme.colorScheme.onSurfaceVariant,
    textAlign = TextAlign.Center,
)
