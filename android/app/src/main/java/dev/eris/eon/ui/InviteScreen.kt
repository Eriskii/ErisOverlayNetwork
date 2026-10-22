package dev.eris.eon.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
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
import androidx.compose.ui.unit.dp
import dev.eris.eon.State
import dev.eris.eon.joinTicketName
import dev.eris.eon.refusal
import kotlinx.coroutines.launch

/**
 * Adding the new device whose own ticket this phone scanned: the person says yes here, the new
 * device's person accepts on its screen, and the phone adds it.
 */
@Composable
fun InviteScreen(ticket: String, state: State, onDone: () -> Unit) {
    val scope = rememberCoroutineScope()
    val name = remember(ticket) { joinTicketName(ticket) }
    var sent by remember { mutableStateOf(false) }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    val inviting = state.inviting?.takeIf { sent && it.name == name }
    BackHandler { onDone() }

    Column(Modifier.fillMaxSize()) {
        Header(title = "Add a device", onBack = onDone)
        Column(Modifier.fillMaxWidth().padding(16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            when {
                name == null -> {
                    Text("That is not a ticket a new device shows.", style = MaterialTheme.typography.bodyMedium)
                    Done(onDone)
                }
                inviting == null -> {
                    Text("Add $name to your devices?", style = MaterialTheme.typography.titleMedium)
                    Text(
                        "Only if you scanned it from $name's own screen. Its person then says yes there.",
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    Problem(error)
                    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End) {
                        TextButton(onClick = onDone, enabled = !busy) { Text("Cancel") }
                        Spacer(Modifier.width(8.dp))
                        Button(
                            onClick = {
                                busy = true
                                error = null
                                scope.launch {
                                    val outcome = command(op("invite").put("ticket", ticket))
                                    busy = false
                                    error = refusal(outcome)
                                    sent = error == null
                                }
                            },
                            enabled = !busy,
                        ) { Text("Add") }
                    }
                }
                inviting.status == "waiting" -> {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        CircularProgressIndicator(Modifier.padding(end = 12.dp))
                        Text("Say yes on $name's screen.", style = MaterialTheme.typography.bodyMedium)
                    }
                }
                inviting.status == "added" -> {
                    Text("$name is added.", style = MaterialTheme.typography.titleMedium)
                    Done(onDone)
                }
                inviting.status == "declined" -> {
                    Text("$name said no.", style = MaterialTheme.typography.titleMedium)
                    Done(onDone)
                }
                else -> {
                    Text("$name was not added.", style = MaterialTheme.typography.titleMedium)
                    Problem(inviting.error)
                    Done(onDone)
                }
            }
        }
    }
}

@Composable
private fun Done(onDone: () -> Unit) {
    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End) {
        Button(onClick = onDone) { Text("Done") }
    }
}
