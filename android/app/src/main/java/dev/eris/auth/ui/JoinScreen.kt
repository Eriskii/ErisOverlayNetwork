package dev.eris.auth.ui

import android.content.Intent
import android.os.Build
import android.provider.MediaStore
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.QrCodeScanner
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.foundation.layout.Spacer
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import dev.eris.auth.State
import dev.eris.auth.refusal
import kotlinx.coroutines.launch

/**
 * A phone in no mesh yet: it starts one, or joins one with a ticket a device in it shows, pasted
 * here or scanned with the camera, which opens the `eris://pair/…` link here. While the request
 * to join waits, both screens show a code to compare.
 */
@Composable
fun JoinScreen(state: State, handed: String?, onHandedTaken: () -> Unit) {
    val scope = rememberCoroutineScope()
    val context = LocalContext.current
    var ticket by remember { mutableStateOf("") }
    var name by remember { mutableStateOf(Build.MODEL ?: "phone") }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }

    LaunchedEffect(handed) {
        if (handed != null) {
            ticket = handed
            onHandedTaken()
        }
    }

    fun send(message: org.json.JSONObject) {
        busy = true
        error = null
        scope.launch {
            error = refusal(command(message))
            busy = false
        }
    }

    Column(Modifier.fillMaxSize()) {
        SecureWindow()
        Header(title = "Your devices", overline = "ErisAuth")
        Column(
            Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            val joining = state.joining
            if (joining != null && joining.error == null) {
                Card(colors = CardDefaults.cardColors(MaterialTheme.colorScheme.secondaryContainer)) {
                    Column(Modifier.fillMaxWidth().padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                        Text("Compare this code with ${joining.host}'s screen", style = MaterialTheme.typography.titleSmall)
                        Text(joining.fingerprint, style = MaterialTheme.typography.headlineMedium, fontFamily = FontFamily.Monospace)
                        Text(
                            "Approve there only if both show the same code. This screen moves on by itself.",
                            style = MaterialTheme.typography.bodySmall,
                        )
                    }
                }
                return@Column
            }

            OutlinedTextField(
                value = name,
                onValueChange = { name = it.take(64) },
                label = { Text("This phone's name") },
                singleLine = true,
                enabled = !busy,
                modifier = Modifier.fillMaxWidth(),
            )

            Text("Join your devices", style = MaterialTheme.typography.titleMedium)
            Text(
                "On a device already among them, choose Add a device, then scan its code with this phone's camera " +
                    "or paste its ticket here.",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            OutlinedTextField(
                value = ticket,
                onValueChange = { ticket = it },
                label = { Text("Ticket") },
                placeholder = { Text("eris://pair/…") },
                minLines = 2,
                maxLines = 4,
                enabled = !busy,
                modifier = Modifier.fillMaxWidth(),
            )
            Problem(error ?: joining?.error)
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                OutlinedButton(
                    onClick = {
                        runCatching { context.startActivity(Intent(MediaStore.INTENT_ACTION_STILL_IMAGE_CAMERA)) }
                            .onFailure { error = "This phone has no camera app to scan with; paste the ticket instead." }
                    },
                    enabled = !busy,
                ) {
                    Icon(Icons.Filled.QrCodeScanner, contentDescription = null, modifier = Modifier.size(18.dp))
                    Spacer(Modifier.width(8.dp))
                    Text("Scan")
                }
                Spacer(Modifier.weight(1f))
                Button(
                    onClick = { send(op("join").put("ticket", ticket.trim()).put("name", name.trim())) },
                    enabled = !busy && ticket.isNotBlank() && name.isNotBlank(),
                ) { Text("Join") }
            }

            HorizontalDivider(Modifier.padding(vertical = 8.dp))
            Text("Or start anew", style = MaterialTheme.typography.titleMedium)
            Text(
                "Start a mesh with this phone as its first device, and add your other devices from here.",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End) {
                OutlinedButton(onClick = { send(op("init").put("name", name.trim())) }, enabled = !busy && name.isNotBlank()) {
                    Text("Start a mesh")
                }
            }
        }
    }
}
