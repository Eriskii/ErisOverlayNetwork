package dev.eris.eon.ui

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Intent
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
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
import dev.eris.eon.Opened
import dev.eris.eon.Qr
import dev.eris.eon.State
import dev.eris.eon.refusal
import dev.eris.eon.remaining
import kotlin.math.floor
import kotlinx.coroutines.launch

/**
 * A ticket, as a QR code for a camera and as text to paste: for a new device to join with, or,
 * with [app], for an app with no ErisOverlayNetwork beside it (one in a browser) to ask with. Once the
 * newcomer asks, its request waits under Requests with a code to compare, and [onDone] hears
 * `true`. Leaving earlier closes the ticket, so nobody can redeem it.
 */
@Composable
fun TicketScreen(app: Boolean, state: State, now: Long, onDone: (asked: Boolean) -> Unit) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var opened by remember { mutableStateOf<Opened?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    // Each new ticket the person asks for.
    var round by remember { mutableIntStateOf(0) }

    LaunchedEffect(round) {
        val answer = command(op(if (app) "add_app" else "add"))
        error = refusal(answer)
        if (error == null) opened = Opened.parse(answer)
    }
    val shown = opened
    LaunchedEffect(state.approvals, shown) {
        if (shown != null && state.approvals.any { it.id == shown.id }) onDone(true)
    }
    fun leave() {
        val id = shown?.id
        scope.launch {
            if (id != null) command(op("close").put("id", id))
            onDone(false)
        }
    }
    BackHandler { leave() }

    Column(Modifier.fillMaxSize()) {
        SecureWindow()
        Header(title = if (app) "Add a browser app" else "Add a device", onBack = { leave() })
        Column(
            Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(14.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            when {
                error != null -> Problem(error)
                shown == null -> CircularProgressIndicator()
                shown.expires <= now -> {
                    Text("This ticket has expired.", style = MaterialTheme.typography.bodyMedium)
                    Button(onClick = {
                        opened = null
                        round += 1
                    }) { Text("Show a new one") }
                }
                else -> {
                    Text(
                        if (app) "Open the app and give it this ticket: scan the code, or paste the text."
                        else "On the new device, scan this QR code with its camera.",
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    QrCode(shown.qr)
                    shown.code?.let { code ->
                        Text("Or type this code on the new device", style = MaterialTheme.typography.bodyMedium)
                        Text(code, style = MaterialTheme.typography.headlineMedium, fontFamily = FontFamily.Monospace)
                    }
                    Text(
                        shown.ticket,
                        style = MaterialTheme.typography.bodySmall,
                        fontFamily = FontFamily.Monospace,
                        maxLines = 2,
                        overflow = TextOverflow.Ellipsis,
                    )
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        OutlinedButton(onClick = {
                            context.getSystemService(ClipboardManager::class.java)
                                .setPrimaryClip(ClipData.newPlainText("ErisOverlayNetwork ticket", shown.ticket))
                        }) { Text("Copy") }
                        OutlinedButton(onClick = {
                            val send = Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(Intent.EXTRA_TEXT, shown.ticket)
                            context.startActivity(Intent.createChooser(send, "Send the ticket"))
                        }) { Text("Share") }
                    }
                    Text(
                        "Good for ${remaining(shown.expires, now)}. The code grants nothing by itself: what asks with " +
                            "it waits under Requests, with a code to compare on both screens.",
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
