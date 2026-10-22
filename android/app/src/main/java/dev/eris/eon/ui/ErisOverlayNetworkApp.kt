package dev.eris.eon.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Apps
import androidx.compose.material.icons.filled.Devices
import androidx.compose.material.icons.filled.Inbox
import androidx.compose.material.icons.filled.Key
import androidx.compose.material3.Badge
import androidx.compose.material3.BadgedBox
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import dev.eris.eon.Native
import dev.eris.eon.State
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.json.JSONObject

/** A command named [name], to fill in. */
fun op(name: String): JSONObject = JSONObject().put("op", name)

/** One command to the daemon, off the main thread. */
suspend fun command(message: JSONObject): JSONObject = withContext(Dispatchers.IO) {
    runCatching { Native.run(message) }
        .getOrElse { JSONObject().put("error", "unreachable").put("message", it.message ?: "ErisOverlayNetwork did not answer") }
}

private enum class Tab { REQUESTS, DEVICES, APPS }

/** What covers the tabs for a while. */
private sealed interface Over {
    /** A ticket for a new device, or with [app] for an app with no eon beside it. */
    data class Ticket(val app: Boolean) : Over

    /** The passphrase of the server of [capability]. */
    data class Passphrase(val capability: String) : Over

    /** Adding the new device whose own [ticket] this phone scanned. */
    data class Invite(val ticket: String) : Over
}

/**
 * ErisOverlayNetwork's screens: starting or joining a mesh, then the requests waiting for the
 * person, the mesh's devices, and the apps this phone approved. A scanned [link] is a ticket: a
 * device's, to join with, or a new device's own, to add it by. The daemon's state is read again
 * whenever it changes, and every second for the clocks.
 */
@Composable
fun ErisOverlayNetworkApp(link: String?, onLinkTaken: () -> Unit) {
    val context = LocalContext.current.applicationContext
    var started by remember { mutableStateOf<Result<Unit>?>(null) }
    var state by remember { mutableStateOf<State?>(null) }
    var now by remember { mutableLongStateOf(System.currentTimeMillis() / 1000) }
    var tab by remember { mutableStateOf(Tab.REQUESTS) }
    var over by remember { mutableStateOf<Over?>(null) }

    LaunchedEffect(Unit) {
        val result = withContext(Dispatchers.IO) { Native.ensureStarted(context) }
        started = result
        if (result.isFailure) return@LaunchedEffect
        var version = -1L
        while (true) {
            val read = command(op("state"))
            if (!read.has("error")) {
                val next = State.parse(read)
                version = next.version
                state = next
            }
            now = System.currentTimeMillis() / 1000
            command(op("wait").put("after", version).put("within_ms", 1000))
        }
    }

    val current = state
    val failure = started?.exceptionOrNull()
    val mesh = current?.mesh
    BackHandler(enabled = over is Over.Passphrase) { over = null }
    BackHandler(enabled = over == null && tab != Tab.REQUESTS && mesh?.member == true) { tab = Tab.REQUESTS }

    when {
        failure != null -> Message("ErisOverlayNetwork did not start", failure.message ?: "")
        current == null -> Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) { CircularProgressIndicator() }
        mesh == null && link?.startsWith("eris://join/") == true -> {
            LaunchedEffect(link) { onLinkTaken() }
            Message(
                "That is another device asking to be added",
                "Scan it with a device already among yours. To join, scan the code a device among yours shows " +
                    "under Add a device.",
            )
        }
        mesh == null -> JoinScreen(state = current, handed = link, onHandedTaken = onLinkTaken)
        !mesh.member -> Message(
            "This phone was removed",
            "Another device removed this phone from the mesh, for good. To join again, clear ErisOverlayNetwork's storage, " +
                "which gives the phone a new key, and join with a ticket from a device in the mesh.",
        )
        else -> {
            // A new device's own ticket asks to add it; a device's ticket has nothing for a phone
            // already in a mesh to join.
            LaunchedEffect(link) {
                if (link != null) {
                    if (link.startsWith("eris://join/")) over = Over.Invite(link)
                    onLinkTaken()
                }
            }
            when (val showing = over) {
                is Over.Ticket -> TicketScreen(
                    app = showing.app,
                    state = current,
                    now = now,
                    onDone = { asked ->
                        over = null
                        if (asked) tab = Tab.REQUESTS
                    },
                )
                is Over.Passphrase -> PassphraseScreen(capability = showing.capability, onBack = { over = null })
                is Over.Invite -> InviteScreen(ticket = showing.ticket, state = current, onDone = { over = null })
                null -> Column(Modifier.fillMaxSize()) {
                    Header(
                        title = current.deviceName ?: "This phone",
                        overline = "ErisOverlayNetwork",
                        actions = {
                            mesh.passphrases.firstOrNull()?.let { capability ->
                                IconButton(onClick = { over = Over.Passphrase(capability) }) {
                                    Icon(Icons.Filled.Key, contentDescription = "Passphrase")
                                }
                            }
                        },
                    )
                    Box(Modifier.weight(1f)) {
                        when (tab) {
                            Tab.REQUESTS -> RequestsScreen(state = current, now = now)
                            Tab.DEVICES -> DevicesScreen(state = current, now = now, onAdd = { over = Over.Ticket(app = false) })
                            Tab.APPS -> AppsScreen(state = current, now = now, onAdd = { over = Over.Ticket(app = true) })
                        }
                    }
                    NavigationBar {
                        NavigationBarItem(
                            selected = tab == Tab.REQUESTS,
                            onClick = { tab = Tab.REQUESTS },
                            icon = {
                                BadgedBox(badge = {
                                    if (current.approvals.isNotEmpty()) Badge { Text("${current.approvals.size}") }
                                }) { Icon(Icons.Filled.Inbox, contentDescription = null) }
                            },
                            label = { Text("Requests") },
                        )
                        NavigationBarItem(
                            selected = tab == Tab.DEVICES,
                            onClick = { tab = Tab.DEVICES },
                            icon = { Icon(Icons.Filled.Devices, contentDescription = null) },
                            label = { Text("Devices") },
                        )
                        NavigationBarItem(
                            selected = tab == Tab.APPS,
                            onClick = { tab = Tab.APPS },
                            icon = { Icon(Icons.Filled.Apps, contentDescription = null) },
                            label = { Text("Apps") },
                        )
                    }
                }
            }
        }
    }
}

/** A screen's title row: back, when there is somewhere to go back to, the title, and actions. */
@Composable
fun Header(title: String, overline: String? = null, onBack: (() -> Unit)? = null, actions: @Composable () -> Unit = {}) {
    Row(
        Modifier.fillMaxWidth().padding(horizontal = 4.dp, vertical = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (onBack != null) {
            IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "back") }
        }
        Column(Modifier.weight(1f).padding(start = if (onBack == null) 16.dp else 0.dp)) {
            overline?.let {
                Text(it, style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
            Text(title, style = MaterialTheme.typography.titleLarge)
        }
        actions()
    }
}

@Composable
fun Message(title: String, body: String) {
    Column(Modifier.fillMaxSize().padding(24.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Text(title, style = MaterialTheme.typography.titleMedium)
        Text(body, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
}

/** A line saying what went wrong, when something did. */
@Composable
fun Problem(text: String?) {
    if (!text.isNullOrEmpty()) {
        Text(text, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.error)
    }
}
