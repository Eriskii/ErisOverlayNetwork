package dev.eris.auth.ui

import android.os.Build
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import dev.eris.auth.Joining
import dev.eris.auth.parseCeiling
import dev.eris.auth.refusalOf
import kotlinx.coroutines.launch
import org.json.JSONArray
import org.json.JSONObject

/**
 * Joining ErisDB, once: a ticket from `erisdb pair` or from a device that has joined, this phone's
 * name, and what it may approve by itself. Then both screens show a code to compare.
 */
@Composable
fun JoinScreen(joining: Joining?, handed: String?, onHandedTaken: () -> Unit) {
    val scope = rememberCoroutineScope()
    var ticket by remember { mutableStateOf("") }
    var name by remember { mutableStateOf(Build.MODEL ?: "phone") }
    var ceiling by remember { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf("") }

    LaunchedEffect(handed) {
        if (handed != null) {
            ticket = handed
            onHandedTaken()
        }
    }

    Column(Modifier.fillMaxSize()) {
        SecureWindow()
        Header(title = "Join ErisDB", overline = "ErisAuth")
        Column(
            Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            val code = joining?.fingerprint
            if (code != null && joining.error == null) {
                Card(colors = CardDefaults.cardColors(MaterialTheme.colorScheme.secondaryContainer)) {
                    Column(Modifier.fillMaxWidth().padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                        Text("Compare this code with the approving screen", style = MaterialTheme.typography.titleSmall)
                        Text(code, style = MaterialTheme.typography.headlineMedium, fontFamily = FontFamily.Monospace)
                        Text(
                            "Approve there only if both show the same code. This screen moves on by itself.",
                            style = MaterialTheme.typography.bodySmall,
                        )
                    }
                }
                return@Column
            }

            Text(
                "Show a ticket on a device that has joined (Add a device), or cut one with erisdb pair, " +
                    "and scan it with the camera or paste it here.",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            OutlinedTextField(
                value = ticket,
                onValueChange = { ticket = it },
                label = { Text("Ticket") },
                placeholder = { Text("erisdb://pair/…") },
                minLines = 2,
                maxLines = 4,
                enabled = !busy,
                modifier = Modifier.fillMaxWidth(),
            )
            OutlinedTextField(
                value = name,
                onValueChange = { name = it.take(64) },
                label = { Text("Name") },
                singleLine = true,
                enabled = !busy,
                modifier = Modifier.fillMaxWidth(),
            )
            OutlinedTextField(
                value = ceiling,
                onValueChange = { ceiling = it },
                label = { Text("Approves by itself") },
                placeholder = { Text("* (everything)") },
                supportingText = {
                    Text("Grants this phone may approve for apps without the passphrase, such as notes:*")
                },
                singleLine = true,
                enabled = !busy,
                modifier = Modifier.fillMaxWidth(),
            )
            val problem = error.ifEmpty { joining?.error ?: "" }
            if (problem.isNotEmpty()) {
                Text(problem, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.error)
            }
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End) {
                Button(
                    onClick = {
                        busy = true
                        error = ""
                        scope.launch {
                            val outcome = command(
                                JSONObject().put("op", "join").put("ticket", ticket.trim()).put("name", name.trim())
                                    .put("ceiling", JSONArray(parseCeiling(ceiling))),
                            )
                            busy = false
                            if (!outcome.optBoolean("ok")) error = refusalOf(outcome)
                        }
                    },
                    enabled = !busy && ticket.isNotBlank() && name.isNotBlank(),
                ) { Text(if (busy) "Reaching ErisDB…" else "Join") }
            }
        }
    }
}
