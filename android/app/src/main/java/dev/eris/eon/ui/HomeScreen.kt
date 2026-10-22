package dev.eris.eon.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.Checkbox
import androidx.compose.material3.MaterialTheme
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
import dev.eris.eon.Prompt
import dev.eris.eon.State
import dev.eris.eon.approveLabel
import dev.eris.eon.listed
import dev.eris.eon.refusal
import dev.eris.eon.remaining
import dev.eris.eon.short
import dev.eris.eon.your
import kotlinx.coroutines.launch

/** What waits for the person: apps on this phone, apps asking by ticket, and new devices. */
@Composable
fun RequestsScreen(state: State, now: Long) {
    LazyColumn(
        Modifier.fillMaxSize(),
        contentPadding = PaddingValues(16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        if (state.approvals.isEmpty()) {
            item {
                Card(colors = CardDefaults.cardColors(MaterialTheme.colorScheme.surfaceVariant)) {
                    Column(Modifier.fillMaxWidth().padding(16.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                        Text("Nothing is waiting", style = MaterialTheme.typography.titleSmall)
                        Text(
                            "When an app on this phone asks to be approved, or a new device or an app asks with a " +
                                "ticket this phone shows, it shows up here.",
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }
            }
        }
        items(state.approvals, key = { it.id }) { prompt ->
            PromptCard(prompt = prompt, now = now)
        }
    }
}

/** One request: who asks, for what, and the person's answer. */
@Composable
fun PromptCard(prompt: Prompt, now: Long) {
    val scope = rememberCoroutineScope()
    var chosen by remember(prompt.id) { mutableStateOf(prompt.asked.toSet()) }
    var passphrase by remember(prompt.id) { mutableStateOf("") }
    var sending by remember(prompt.id) { mutableStateOf(false) }
    var error by remember(prompt.id) { mutableStateOf<String?>(null) }
    val busy = sending || prompt.busy

    fun answer(approve: Boolean) {
        sending = true
        error = null
        scope.launch {
            val outcome = command(if (approve) prompt.decide(chosen, passphrase) else op("deny").put("id", prompt.id))
            sending = false
            error = refusal(outcome)
            if (outcome.optString("error") == "wrong_passphrase") passphrase = ""
        }
    }

    Card(colors = CardDefaults.cardColors(MaterialTheme.colorScheme.surfaceVariant), modifier = Modifier.fillMaxWidth()) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            Row(verticalAlignment = Alignment.Top) {
                Column(Modifier.weight(1f)) {
                    Text(prompt.name, style = MaterialTheme.typography.titleMedium)
                    Text(prompt.asks, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
                Text(remaining(prompt.expires, now), style = MaterialTheme.typography.labelMedium)
            }

            if (prompt.fingerprint != null) {
                Surface(color = MaterialTheme.colorScheme.secondaryContainer, shape = MaterialTheme.shapes.medium) {
                    Column(Modifier.fillMaxWidth().padding(12.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                        Text(
                            if (prompt.isDevice) "Compare with the new device" else "Compare with the app",
                            style = MaterialTheme.typography.labelMedium,
                        )
                        Text(prompt.fingerprint, style = MaterialTheme.typography.headlineSmall, fontFamily = FontFamily.Monospace)
                        Text("Approve only if both screens show this code.", style = MaterialTheme.typography.bodySmall)
                    }
                }
            }

            if (!prompt.isDevice) {
                Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
                    when {
                        prompt.packageName != null -> {
                            Detail("App", prompt.packageName)
                            Detail("Signed", prompt.certificate?.let { short(it, 8, 8) } ?: "unknown")
                        }
                        prompt.program != null -> Detail("Program", prompt.program)
                        else -> Detail("App", prompt.who)
                    }
                    Detail("Key", short(prompt.key))
                }
            }

            val single = prompt.serves.filter { it.single }.map { your(it.capability) }
            if (single.isNotEmpty()) {
                val replaced = prompt.replaces.map { it.second }
                Text(
                    "Every app that uses ${listed(single)} will use it" +
                        (if (replaced.isEmpty()) "." else ", in place of ${listed(replaced)}."),
                    style = MaterialTheme.typography.bodyMedium,
                )
            }

            prompt.uses.filterValues { it.isNotEmpty() }.forEach { (capability, grants) ->
                Text("It wants, in ${your(capability)}", style = MaterialTheme.typography.labelLarge)
                grants.forEach { grant ->
                    val choice = capability to grant
                    Row(
                        Modifier.fillMaxWidth().clickable(enabled = !busy) {
                            chosen = if (choice in chosen) chosen - choice else chosen + choice
                            error = null
                        },
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Checkbox(checked = choice in chosen, onCheckedChange = null, enabled = !busy)
                        Text(
                            grant,
                            style = MaterialTheme.typography.bodyMedium,
                            fontFamily = FontFamily.Monospace,
                            modifier = Modifier.padding(start = 8.dp),
                        )
                    }
                }
            }

            if (prompt.passphrase) {
                SecureWindow()
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
            Problem(error)
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End) {
                TextButton(onClick = { answer(false) }, enabled = !busy) { Text("Deny") }
                Spacer(Modifier.width(8.dp))
                Button(
                    onClick = { answer(true) },
                    enabled = !busy &&
                        (prompt.asked.isEmpty() || chosen.isNotEmpty() || prompt.serves.isNotEmpty()) &&
                        (!prompt.passphrase || passphrase.isNotEmpty()),
                ) {
                    Text(approveLabel(picked = prompt.asked.count { it in chosen }, of = prompt.asked.size))
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
