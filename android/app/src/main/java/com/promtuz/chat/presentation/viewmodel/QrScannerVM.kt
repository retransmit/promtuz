package com.promtuz.chat.presentation.viewmodel

import android.app.Application
import android.content.Context
import androidx.camera.core.ImageAnalysis
import androidx.camera.lifecycle.ProcessCameraProvider
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.google.mlkit.vision.barcode.common.Barcode
import com.promtuz.chat.presentation.state.PermissionState
import com.promtuz.core.CoreBridge
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import timber.log.Timber
import uniffi.core.CoreException

class QrScannerVM(
    private val application: Application
) : ViewModel() {
    private val context: Context get() = application.applicationContext
    private val log = Timber.tag("QrScannerVM")

    var imageAnalysis =
        ImageAnalysis.Builder().setBackpressureStrategy(ImageAnalysis.STRATEGY_KEEP_ONLY_LATEST)
            .build()

    private val _isCameraAvailable = MutableStateFlow(false)
    val isCameraAvailable = _isCameraAvailable.asStateFlow()

    private val _cameraPermissionState = MutableStateFlow(PermissionState.NotRequested)
    val cameraPermissionState = _cameraPermissionState.asStateFlow()

    private val _cameraProviderState = MutableStateFlow<ProcessCameraProvider?>(null)
    val cameraProviderState = _cameraProviderState.asStateFlow()

    private val _scanError = MutableStateFlow<String?>(null)
    val scanError = _scanError.asStateFlow()

    /** Flips true once a valid invite is accepted; the screen closes on it. */
    private val _paired = MutableStateFlow(false)
    val paired = _paired.asStateFlow()

    // Guards against the analyzer re-firing pairFromQr while one is in flight.
    // The ML Kit analyzer delivers frames serially, so a plain flag suffices.
    @Volatile
    private var processing = false

    fun clearScanError() {
        _scanError.value = null
    }

    fun setCameraProvider(provider: ProcessCameraProvider) {
        _cameraProviderState.value = provider
    }

    fun handleCameraPermissionRequest(isGranted: Boolean) {
        if (isGranted) {
            _cameraPermissionState.value = PermissionState.Granted
        } else {
            _cameraPermissionState.value = PermissionState.Denied
        }
    }

    fun makeCameraAvailable() {
        _isCameraAvailable.value = true
    }

    fun handleScannedBarcodes(barcodes: List<Barcode>) {
        // One accepted invite per scan session: ignore re-detections of the
        // same QR held in frame (analyzer ticks ~2x/s) and anything after a
        // successful pair. A bad QR re-arms so the user can try another.
        if (processing || _paired.value) return
        val bytes = barcodes.firstNotNullOfOrNull { it.rawBytes } ?: return
        processing = true
        viewModelScope.launch {
            try {
                CoreBridge.pairFromQr(bytes)
                imageAnalysis.clearAnalyzer() // valid invite accepted — stop scanning
                _paired.value = true
            } catch (e: CoreException) {
                log.e(e, "pairFromQr failed")
                _scanError.value = e.message ?: "Invalid QR code"
            } finally {
                processing = false
            }
        }
    }
}
