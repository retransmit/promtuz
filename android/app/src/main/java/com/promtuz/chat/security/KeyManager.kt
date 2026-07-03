package com.promtuz.chat.security

import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyInfo
import android.security.keystore.KeyProperties
import android.security.keystore.StrongBoxUnavailableException
import timber.log.Timber
import uniffi.core.CoreException
import uniffi.core.SecureStore
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.SecretKeyFactory
import javax.crypto.spec.GCMParameterSpec

/**
 * Android Keystore custody for libcore's [SecureStore] port. Seals/opens key
 * material with a hardware-backed AES-256-GCM key; only *custody* of the
 * wrapping key crosses the FFI — the actual crypto stays in core.
 *
 * Blob layout: `[iv:12][ciphertext+tag:16]`.
 *
 * ponytail: no setUnlockedDeviceRequired(true). core opens the sealed secret
 * from Identity::get() on background QUIC threads (data/identity.rs), which
 * fire while the device is locked — an unlocked-device key would make those
 * throw and silently kill background reconnect/messaging. Upgrade path: once
 * core caches the opened secret in memory (opens once, foreground), add it.
 */
object KeyManager : SecureStore {
    private const val KEY_ALIAS = "master_key"
    private const val ANDROID_KEYSTORE = "AndroidKeyStore"
    private const val TRANSFORMATION = "AES/GCM/NoPadding"

    private val keyStore = KeyStore.getInstance(ANDROID_KEYSTORE).apply { load(null) }

    @Synchronized
    private fun getOrCreateKey(): SecretKey {
        (keyStore.getKey(KEY_ALIAS, null) as? SecretKey)?.let { return it }
        // StrongBox where available, graceful fallback to the TEE. Rebuild the
        // FULL spec on fallback so no hardening is silently dropped.
        val key = try {
            newKey(strongBox = true)
        } catch (_: StrongBoxUnavailableException) {
            newKey(strongBox = false)
        }
        logSecurityLevel(key)
        return key
    }

    private fun newKey(strongBox: Boolean): SecretKey {
        val spec = KeyGenParameterSpec.Builder(
            KEY_ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT,
        ).apply {
            setBlockModes(KeyProperties.BLOCK_MODE_GCM)
            setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
            setKeySize(256)
            setUserAuthenticationRequired(false) // headless seal/open — no biometric prompt
            if (strongBox) setIsStrongBoxBacked(true)
        }.build()
        return KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, ANDROID_KEYSTORE)
            .apply { init(spec) }.generateKey()
    }

    /** Log the actual hardware level we got (StrongBox / TEE / software). */
    private fun logSecurityLevel(key: SecretKey) {
        runCatching {
            val info = SecretKeyFactory.getInstance(key.algorithm, ANDROID_KEYSTORE)
                .getKeySpec(key, KeyInfo::class.java) as KeyInfo
            Timber.tag("KeyManager").i("identity key securityLevel=${info.securityLevel}")
        }
    }

    override fun seal(plaintext: ByteArray): ByteArray = try {
        val cipher = Cipher.getInstance(TRANSFORMATION).apply {
            init(Cipher.ENCRYPT_MODE, getOrCreateKey())
        }
        cipher.iv + cipher.doFinal(plaintext) // [iv:12][ct+tag]
    } catch (e: Exception) {
        throw CoreException.Internal("seal: ${e.message}")
    }

    override fun open(ciphertext: ByteArray): ByteArray = try {
        val iv = ciphertext.copyOfRange(0, 12)
        val body = ciphertext.copyOfRange(12, ciphertext.size)
        val cipher = Cipher.getInstance(TRANSFORMATION).apply {
            init(Cipher.DECRYPT_MODE, getOrCreateKey(), GCMParameterSpec(128, iv))
        }
        cipher.doFinal(body)
    } catch (e: Exception) {
        throw CoreException.Internal("open: ${e.message}")
    }
}
