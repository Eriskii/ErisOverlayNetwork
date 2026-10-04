package dev.eris.auth.ui

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Intent
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import dev.eris.auth.Qr
import dev.eris.auth.refusalOf
import dev.eris.auth.remaining
import kotlin.math.floor
import org.json.JSONObject

/**
 * A ticket a new device joins with, as a QR code for its camera and as text to paste. Once the
 * new device asks, its request shows on the home screen with a code to compare.
 */
@Composable
fun AddDeviceScreen(waiting: List<String>, now: Long, onBack: () -> Unit) {
    val context = LocalContext.current
    var added by remember { mutableStateOf<JSONObject?>(null) }
    var error by remember { mutableStateOf("") }

    LaunchedEffect(Unit) {
        val answer = command(JSONObject().put("op", "add"))
        if (answer.has("ticket")) added = answer else error = refusalOf(answer)
    }
    LaunchedEffect(waiting, added) {
        if (added?.optString("id") in waiting) onBack()
    }

    Column(Modifier.fillMaxSize()) {
        SecureWindow()
        Header(title = "Add a device", onBack = onBack)
        Column(
            Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(14.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            val ticket = added?.optString("ticket")
            val qr = added?.optJSONObject("qr")?.let(Qr::parse)
            when {
                error.isNotEmpty() -> Text(error, color = MaterialTheme.colorScheme.error)
                ticket == null || qr == null -> CircularProgressIndicator()
                else -> {
                    Text(
                        "Scan this with the new device's camera, or run on it:",
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    QrCode(qr)
                    Text(
                        "erisauth join '$ticket'",
                        style = MaterialTheme.typography.bodySmall,
                        fontFamily = FontFamily.Monospace,
                        maxLines = 2,
                        overflow = TextOverflow.Ellipsis,
                    )
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        OutlinedButton(onClick = {
                            context.getSystemService(ClipboardManager::class.java)
                                .setPrimaryClip(ClipData.newPlainText("ErisDB ticket", ticket))
                        }) { Text("Copy") }
                        OutlinedButton(onClick = {
                            val send = Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(Intent.EXTRA_TEXT, ticket)
                            context.startActivity(Intent.createChooser(send, "Send the ticket"))
                        }) { Text("Share") }
                    }
                    Text(
                        "Good for ${remaining(added?.optLong("expires") ?: 0, now)}. Its request appears here with a " +
                            "code to compare; approve it once both screens match.",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
        }
    }
}

/** The code on white, dark modules black, snapped to whole pixels so no seams show between them. */
@Composable
private fun QrCode(qr: Qr) {
    Box(Modifier.size(260.dp).clip(RoundedCornerShape(12.dp)).background(Color.White)) {
        Canvas(Modifier.fillMaxSize()) {
            val cell = floor(size.minDimension / qr.side)
            val origin = (size.minDimension - cell * qr.side) / 2
            for (y in 0 until qr.side) {
                for (x in 0 until qr.side) {
                    if (qr.dark(x, y)) {
                        drawRect(Color.Black, topLeft = Offset(origin + x * cell, origin + y * cell), size = Size(cell, cell))
                    }
                }
            }
        }
    }
}
