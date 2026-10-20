package dev.eris.auth.ui

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
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
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
import androidx.compose.ui.unit.dp
import dev.eris.auth.Note
import dev.eris.auth.State
import dev.eris.auth.ago
import dev.eris.auth.refusal
import dev.eris.auth.serviceName
import kotlinx.coroutines.launch

/** The mesh's ErisDB, the apps this phone approved, and a ticket for an app with no ErisAuth beside it. */
@Composable
fun AppsScreen(state: State, now: Long, onAdd: () -> Unit) {
    var revoking by remember { mutableStateOf<Note?>(null) }
    LazyColumn(
        Modifier.fillMaxSize(),
        contentPadding = PaddingValues(16.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        state.mesh?.erisdb?.let { erisdb ->
            item {
                Card(colors = CardDefaults.cardColors(MaterialTheme.colorScheme.secondaryContainer)) {
                    Column(Modifier.fillMaxWidth().padding(16.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                        Text("Your ErisDB", style = MaterialTheme.typography.labelMedium)
                        Text(erisdb.name, style = MaterialTheme.typography.titleSmall)
                        Text(
                            "approved on ${erisdb.device}, ${if (erisdb.reachable) "reachable" else "not reachable"}",
                            style = MaterialTheme.typography.bodySmall,
                        )
                    }
                }
            }
        }
        if (state.notes.isEmpty()) {
            item {
                Text(
                    "Apps this phone approves show up here, and can be revoked here.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
        items(state.notes, key = { it.id }) { note ->
            Card(colors = CardDefaults.cardColors(MaterialTheme.colorScheme.surfaceVariant), modifier = Modifier.fillMaxWidth()) {
                Column(Modifier.fillMaxWidth().padding(16.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Column(Modifier.weight(1f)) {
                            Text(note.name, style = MaterialTheme.typography.titleSmall)
                            Text(
                                (if (note.serves) "serves ${serviceName(note.service)}" else "uses ${serviceName(note.service)}") +
                                    ", approved ${ago(note.at, now)}",
                                style = MaterialTheme.typography.bodySmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                        if (note.standing.isNotEmpty()) {
                            Text(note.standing, style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.tertiary)
                        }
                    }
                    if (note.grants.isNotEmpty()) {
                        Text(note.grants.joinToString(", "), style = MaterialTheme.typography.bodySmall, fontFamily = FontFamily.Monospace)
                    }
                    note.program?.let {
                        Text(it, style = MaterialTheme.typography.bodySmall, fontFamily = FontFamily.Monospace)
                    }
                    if (!note.revoked) {
                        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End) {
                            TextButton(onClick = { revoking = note }) { Text("Revoke", color = MaterialTheme.colorScheme.error) }
                        }
                    }
                }
            }
        }
        item {
            OutlinedButton(onClick = onAdd, modifier = Modifier.fillMaxWidth().padding(top = 4.dp)) {
                Icon(Icons.Filled.Add, contentDescription = null, modifier = Modifier.size(18.dp))
                Spacer(Modifier.width(8.dp))
                Text("Add a browser app")
            }
        }
    }
    revoking?.let { note -> RevokeDialog(note = note, onClose = { revoking = null }) }
}

@Composable
private fun RevokeDialog(note: Note, onClose: () -> Unit) {
    val scope = rememberCoroutineScope()
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    AlertDialog(
        onDismissRequest = onClose,
        title = { Text("Revoke ${note.name}?") },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
                Text(
                    if (note.erisdb) "It stops being your ErisDB, and stops counting on every device."
                    else "Its approval stops counting on every device, for good.",
                )
                Problem(error)
            }
        },
        confirmButton = {
            TextButton(
                onClick = {
                    busy = true
                    scope.launch {
                        val outcome = command(op("revoke").put("note", note.id))
                        busy = false
                        error = refusal(outcome)
                        if (error == null) onClose()
                    }
                },
                enabled = !busy,
            ) { Text("Revoke", color = MaterialTheme.colorScheme.error) }
        },
        dismissButton = { TextButton(onClick = onClose) { Text("Cancel") } },
    )
}
