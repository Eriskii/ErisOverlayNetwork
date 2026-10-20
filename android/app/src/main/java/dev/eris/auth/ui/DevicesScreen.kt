package dev.eris.auth.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
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
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.Checkbox
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
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
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp
import dev.eris.auth.Device
import dev.eris.auth.Removal
import dev.eris.auth.State
import dev.eris.auth.ago
import dev.eris.auth.refusal
import kotlinx.coroutines.launch
import org.json.JSONArray

/** The mesh's devices: who added each, whether it answers; rename one, or remove it with the devices it added. */
@Composable
fun DevicesScreen(state: State, now: Long, onAdd: () -> Unit) {
    val devices = state.mesh?.devices.orEmpty()
    var chosen by remember { mutableStateOf<Device?>(null) }
    LazyColumn(
        Modifier.fillMaxSize(),
        contentPadding = PaddingValues(16.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        items(devices, key = { it.id }) { device ->
            Card(
                colors = CardDefaults.cardColors(MaterialTheme.colorScheme.surfaceVariant),
                modifier = Modifier.fillMaxWidth().clickable { chosen = device },
            ) {
                Row(Modifier.fillMaxWidth().padding(16.dp), verticalAlignment = Alignment.CenterVertically) {
                    Reachable(device.reachable)
                    Spacer(Modifier.width(12.dp))
                    Column(Modifier.weight(1f)) {
                        Text(
                            device.name + if (device.isThis) " (this phone)" else "",
                            style = MaterialTheme.typography.titleSmall,
                        )
                        Text(
                            device.addedByName?.let { "added by $it ${ago(device.addedAt, now)}" }
                                ?: "started the mesh ${ago(device.addedAt, now)}",
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }
            }
        }
        item {
            OutlinedButton(onClick = onAdd, modifier = Modifier.fillMaxWidth().padding(top = 4.dp)) {
                Icon(Icons.Filled.Add, contentDescription = null, modifier = Modifier.size(18.dp))
                Spacer(Modifier.width(8.dp))
                Text("Add a device")
            }
        }
        if (state.events.isNotEmpty()) {
            item { Text("Lately", style = MaterialTheme.typography.labelLarge, modifier = Modifier.padding(top = 12.dp)) }
            items(state.events.reversed(), key = { "event ${it.seq}" }) { event ->
                Text(
                    "${event.text}, ${ago(event.at, now)}",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }
    chosen?.let { device -> DeviceDialog(device = device, onClose = { chosen = null }) }
}

/** Green when it answered when last asked, red when it did not, grey before it has been asked. */
@Composable
private fun Reachable(reachable: Boolean?) {
    val color = when (reachable) {
        true -> Color(0xFF3DBE6B)
        false -> MaterialTheme.colorScheme.error
        null -> MaterialTheme.colorScheme.outline
    }
    Box(Modifier.size(10.dp).clip(CircleShape).background(color))
}

/** Rename [device], or start removing it. */
@Composable
private fun DeviceDialog(device: Device, onClose: () -> Unit) {
    val scope = rememberCoroutineScope()
    var name by remember(device.id) { mutableStateOf(device.name) }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    var removal by remember { mutableStateOf<Removal?>(null) }

    removal?.let { offer ->
        RemovalDialog(removal = offer, leaving = device.isThis, onClose = onClose)
        return
    }
    AlertDialog(
        onDismissRequest = onClose,
        title = { Text(device.name) },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                OutlinedTextField(
                    value = name,
                    onValueChange = { name = it.take(64) },
                    label = { Text("Name") },
                    singleLine = true,
                    enabled = !busy,
                    modifier = Modifier.fillMaxWidth(),
                )
                Problem(error)
                TextButton(
                    onClick = {
                        busy = true
                        scope.launch {
                            val offer = command(op("removal").put("device", device.id))
                            busy = false
                            error = refusal(offer)
                            if (error == null) removal = Removal.parse(offer)
                        }
                    },
                    enabled = !busy,
                ) { Text(if (device.isThis) "Remove this phone…" else "Remove…", color = MaterialTheme.colorScheme.error) }
            }
        },
        confirmButton = {
            TextButton(
                onClick = {
                    busy = true
                    scope.launch {
                        val outcome = command(op("rename").put("device", device.id).put("name", name.trim()))
                        busy = false
                        error = refusal(outcome)
                        if (error == null) onClose()
                    }
                },
                enabled = !busy && name.isNotBlank() && name.trim() != device.name,
            ) { Text("Rename") }
        },
        dismissButton = { TextButton(onClick = onClose) { Text("Close") } },
    )
}

/** Removing a device: the devices it added go with it, each ticked; the person unticks any to keep. */
@Composable
private fun RemovalDialog(removal: Removal, leaving: Boolean, onClose: () -> Unit) {
    val scope = rememberCoroutineScope()
    var kept by remember { mutableStateOf(setOf<String>()) }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    AlertDialog(
        onDismissRequest = onClose,
        title = { Text("Remove ${removal.device.name}?") },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
                Text(
                    if (leaving) "This phone leaves the mesh for good; to come back it joins again with a new key."
                    else "${removal.device.name} leaves the mesh for good, and the apps it approved stop working.",
                    style = MaterialTheme.typography.bodyMedium,
                )
                if (removal.also.isNotEmpty()) {
                    Text("It added these devices. They go too, unless you untick them:", style = MaterialTheme.typography.bodyMedium)
                    removal.also.forEach { added ->
                        Row(
                            Modifier.fillMaxWidth().clickable(enabled = !busy) {
                                kept = if (added.id in kept) kept - added.id else kept + added.id
                            },
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Checkbox(checked = added.id !in kept, onCheckedChange = null, enabled = !busy)
                            Text(added.name, modifier = Modifier.padding(start = 8.dp))
                        }
                    }
                }
                Problem(error)
            }
        },
        confirmButton = {
            TextButton(
                onClick = {
                    busy = true
                    scope.launch {
                        val outcome = command(op("remove").put("devices", JSONArray(removal.devices(kept))))
                        busy = false
                        error = refusal(outcome)
                        if (error == null) onClose()
                    }
                },
                enabled = !busy,
            ) { Text("Remove ${removal.devices(kept).size}", color = MaterialTheme.colorScheme.error) }
        },
        dismissButton = { TextButton(onClick = onClose) { Text("Cancel") } },
    )
}
