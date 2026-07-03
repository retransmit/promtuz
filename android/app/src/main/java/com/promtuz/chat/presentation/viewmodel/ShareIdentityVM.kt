package com.promtuz.chat.presentation.viewmodel

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.promtuz.core.CoreBridge
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import timber.log.Timber
import uniffi.core.CoreException

class ShareIdentityVM : ViewModel() {
    private var _qrData = MutableStateFlow<ByteArray?>(null)
    val qrData = _qrData.asStateFlow()

    fun setQR(qr: ByteArray) {
        _qrData.value = qr
    }

    init {
        viewModelScope.launch {
            try {
                setQR(CoreBridge.makeInviteQr())
            } catch (e: CoreException) {
                Timber.tag("ShareIdentityVM").e(e, "makeInviteQr failed")
            }
        }
    }
}
