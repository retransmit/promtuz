package com.promtuz.chat.ui.activities

import android.os.Bundle
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.appcompat.app.AppCompatActivity
import com.promtuz.chat.presentation.viewmodel.ShareIdentityVM
import com.promtuz.chat.ui.screens.ShareIdentityScreen
import com.promtuz.chat.ui.theme.PromtuzTheme
import org.koin.androidx.viewmodel.ext.android.viewModel

class ShareIdentity : AppCompatActivity() {
    private val viewModel: ShareIdentityVM by viewModel()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        enableEdgeToEdge()

        setContent {
            PromtuzTheme {
                ShareIdentityScreen(viewModel)
            }
        }
    }
}
