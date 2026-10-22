package dev.eris.eon

import android.Manifest
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import dev.eris.eon.ui.ErisOverlayNetworkApp
import dev.eris.eon.ui.ErisOverlayNetworkTheme

/**
 * ErisOverlayNetwork's screens. A ticket scanned with the camera opens here: a device's,
 * `eris://pair/…`, fills in the join screen; a new device's own, `eris://join/…`, asks to add it.
 */
class MainActivity : ComponentActivity() {
    /** A ticket waiting for a screen to take it. */
    private var ticket by mutableStateOf<String?>(null)

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        ticket = ticketIn(intent)
        if (checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED) {
            requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS), 0)
        }
        Notifications.channels(this)
        Watcher.start(this)
        setContent {
            ErisOverlayNetworkTheme {
                Surface(modifier = Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.background) {
                    Box(Modifier.safeDrawingPadding()) {
                        ErisOverlayNetworkApp(link = ticket, onLinkTaken = { ticket = null })
                    }
                }
            }
        }
    }

    override fun onResume() {
        super.onResume()
        Watcher.visible = true
    }

    override fun onPause() {
        Watcher.visible = false
        super.onPause()
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        ticketIn(intent)?.let { ticket = it }
    }

    private fun ticketIn(intent: Intent?): String? =
        intent?.data?.takeIf { it.scheme == "eris" && (it.host == "pair" || it.host == "join") }?.toString()
}
