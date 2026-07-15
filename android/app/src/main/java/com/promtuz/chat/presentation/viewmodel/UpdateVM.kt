package com.promtuz.chat.presentation.viewmodel

import androidx.lifecycle.ViewModel
import com.promtuz.chat.update.UpdateManifest
import com.promtuz.chat.update.UpdateRepository
import java.io.File

class UpdateVM(private val updates: UpdateRepository) : ViewModel() {
    val state = updates.state
    fun check() = updates.check()
    fun download(manifest: UpdateManifest) = updates.download(manifest)
    fun cancelDownload() = updates.cancelDownload()
    fun install(manifest: UpdateManifest, apk: File) = updates.install(manifest, apk)
    fun requestInstallPermission() = updates.requestInstallPermission()
}
