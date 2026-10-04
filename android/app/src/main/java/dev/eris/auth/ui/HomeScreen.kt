package dev.eris.auth.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Devices
import androidx.compose.material.icons.filled.Key
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.Checkbox
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import dev.eris.auth.Approval
import dev.eris.auth.DeviceState
import dev.eris.auth.approveLabel
import dev.eris.auth.covers
import dev.eris.auth.needsPassphrase
import dev.eris.auth.refusalOf
import dev.eris.auth.remaining
import kotlinx.coroutines.launch
import org.json.JSONArray
import org.json.JSONObject

/** The requests waiting for the person, and the way to add a device or look after the passphrase. */
@Composable
fun HomeScreen(
    state: DeviceState,
    approvals: List<Approval>,
    now: Long,
    onAnswered: (String) -> Unit,
    onAddDevice: () -> Unit,
    onPassphrase: () -> Unit,
) {
    Column(Modifier.fillMaxSize()) {
        Header(title = state.name ?: "This device", overline = "ErisAuth")
        LazyColumn(
            Modifier.fillMaxSize(),
            contentPadding = PaddingValues(16.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            if (approvals.isEmpty()) {
                item { NothingWaiting(state) }
            }
            items(approvals, key = { it.id }) { approval ->
                ApprovalCard(approval = approval, device = state.grants, now = now, onAnswered = onAnswered)
            }
            item {
                Row(Modifier.fillMaxWidth().padding(top = 4.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    OutlinedButton(onClick = onAddDevice, modifier = Modifier.weight(1f)) {
                        Icon(Icons.Filled.Devices, contentDescription = null, modifier = Modifier.size(18.dp))
                        Spacer(Modifier.width(8.dp))
                        Text("Add a device")
                    }
                    OutlinedButton(onClick = onPassphrase, modifier = Modifier.weight(1f)) {
                        Icon(Icons.Filled.Key, contentDescription = null, modifier = Modifier.size(18.dp))
                        Spacer(Modifier.width(8.dp))
                        Text("Passphrase")
                    }
                }
            }
        }
    }
}

@Composable
private fun NothingWaiting(state: DeviceState) {
    Card(colors = CardDefaults.cardColors(MaterialTheme.colorScheme.surfaceVariant)) {
        Column(Modifier.fillMaxWidth().padding(16.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Text("Nothing is waiting", style = MaterialTheme.typography.titleSmall)
            Text(
                "When an app on this phone asks to sign in, or a new device asks to join, it shows up here.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Text(
                "Approves by itself: " +
                    state.approvesItself.joinToString(", ").ifEmpty { "nothing; everything takes the passphrase" },
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

/** One request: who asks, what for, and the person's answer. */
@Composable
fun ApprovalCard(approval: Approval, device: List<String>, now: Long, onAnswered: (String) -> Unit) {
    val scope = rememberCoroutineScope()
    var chosen by remember(approval.id) { mutableStateOf(approval.optional.toSet()) }
    var passphrase by remember(approval.id) { mutableStateOf("") }
    var busy by remember(approval.id) { mutableStateOf(false) }
    var error by remember(approval.id) { mutableStateOf("") }
    val granted = approval.granted(chosen)
    val needs = needsPassphrase(device, granted)
    val service = if (approval.service == "erisdb") "ErisDB" else approval.service

    fun answer(approve: Boolean) {
        busy = true
        error = ""
        scope.launch {
            val message = if (approve) {
                JSONObject().put("op", "decide").put("id", approval.id).put("granted", JSONArray(granted))
                    .apply { if (needs) put("passphrase", passphrase) }
            } else {
                JSONObject().put("op", "deny").put("id", approval.id)
            }
            val outcome = command(message)
            busy = false
            if (outcome.optBoolean("ok")) {
                onAnswered(approval.id)
            } else {
                error = refusalOf(outcome)
                if (outcome.optString("error") == "wrong_passphrase") passphrase = ""
            }
        }
    }

    Card(
        colors = CardDefaults.cardColors(MaterialTheme.colorScheme.surfaceVariant),
        modifier = Modifier.fillMaxWidth(),
    ) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            Row(verticalAlignment = Alignment.Top) {
                Column(Modifier.weight(1f)) {
                    Text(approval.client, style = MaterialTheme.typography.titleMedium)
                    Text(
                        if (approval.isDevice) "asks to join $service" else "asks to use $service",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                Text(remaining(approval.expires, now), style = MaterialTheme.typography.labelMedium)
            }

            if (approval.isDevice) {
                Surface(color = MaterialTheme.colorScheme.secondaryContainer, shape = MaterialTheme.shapes.medium) {
                    Column(Modifier.fillMaxWidth().padding(12.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                        Text("Compare with the new device", style = MaterialTheme.typography.labelMedium)
                        Text(
                            approval.fingerprint ?: "?",
                            style = MaterialTheme.typography.headlineSmall,
                            fontFamily = FontFamily.Monospace,
                        )
                        Text("Approve only if both screens show this code.", style = MaterialTheme.typography.bodySmall)
                    }
                }
            } else {
                Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
                    if (approval.packageName != null) {
                        Detail("App", approval.packageName)
                        Detail("Signed", approval.certificate?.let { short(it, 8, 8) } ?: "unknown")
                    } else {
                        Detail("Program", (approval.executable ?: "unknown") + (approval.pid?.let { "  (pid $it)" } ?: ""))
                    }
                    Detail("Key", listOfNotNull(approval.identityKind, approval.identityKey?.let { short(it, 8, 6) }).joinToString(" ").ifEmpty { "unknown" })
                }
            }

            Text("It wants", style = MaterialTheme.typography.labelLarge)
            if (approval.essential.isNotEmpty()) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Checkbox(checked = true, onCheckedChange = null, enabled = false)
                    Column(Modifier.padding(start = 8.dp)) {
                        Text("Act as an ErisAuth device", style = MaterialTheme.typography.bodyMedium)
                        Text(
                            "approve pairings, use the passphrase",
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }
            }
            approval.optional.forEach { grant ->
                Row(
                    Modifier.fillMaxWidth().clickable(enabled = !busy) {
                        chosen = if (grant in chosen) chosen - grant else chosen + grant
                        error = ""
                    },
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Checkbox(checked = grant in chosen, onCheckedChange = null, enabled = !busy)
                    Text(
                        grant,
                        style = MaterialTheme.typography.bodyMedium,
                        fontFamily = FontFamily.Monospace,
                        modifier = Modifier.weight(1f).padding(start = 8.dp),
                    )
                    if (device.none { covers(it, grant) }) {
                        Text("passphrase", style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.tertiary)
                    }
                }
            }

            if (needs && granted.isNotEmpty()) {
                OutlinedTextField(
                    value = passphrase,
                    onValueChange = { passphrase = it },
                    label = { Text("Passphrase") },
                    singleLine = true,
                    enabled = !busy,
                    visualTransformation = PasswordVisualTransformation(),
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password),
                    modifier = Modifier.fillMaxWidth(),
                )
            }
            if (error.isNotEmpty()) {
                Text(error, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.error)
            }
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End) {
                TextButton(onClick = { answer(false) }, enabled = !busy) { Text("Deny") }
                Spacer(Modifier.width(8.dp))
                Button(
                    onClick = { answer(true) },
                    enabled = !busy && granted.isNotEmpty() && (!needs || passphrase.isNotEmpty()),
                ) {
                    Text(approveLabel(picked = approval.optional.count { it in chosen }, of = approval.optional.size))
                }
            }
        }
    }
}

@Composable
private fun Detail(label: String, value: String) {
    Row {
        Text(
            label,
            style = MaterialTheme.typography.labelMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.width(64.dp),
        )
        Text(value, style = MaterialTheme.typography.bodySmall, fontFamily = FontFamily.Monospace)
    }
}

private fun short(text: String, head: Int, tail: Int): String =
    if (text.length <= head + tail + 1) text else "${text.take(head)}…${text.takeLast(tail)}"
