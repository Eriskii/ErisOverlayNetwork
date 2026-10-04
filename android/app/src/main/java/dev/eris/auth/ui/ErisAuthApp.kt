package dev.eris.auth.ui

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
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
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
import dev.eris.auth.DeviceState
import dev.eris.auth.Native
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.withContext
import org.json.JSONObject

/** One command to the daemon, off the main thread. */
suspend fun command(message: JSONObject): JSONObject = withContext(Dispatchers.IO) {
    runCatching { Native.run(message) }
        .getOrElse { JSONObject().put("error", "unreachable").put("message", it.message ?: "ErisAuth did not answer") }
}

private enum class Screen { HOME, ADD_DEVICE, PASSPHRASE }

/**
 * ErisAuth's screens: joining ErisDB, then the requests waiting for the person, adding a device,
 * and the passphrase. What the daemon holds is read again every second.
 */
@Composable
fun ErisAuthApp(ticket: String?, onTicketTaken: () -> Unit) {
    val context = LocalContext.current.applicationContext
    var started by remember { mutableStateOf<Result<Unit>?>(null) }
    var state by remember { mutableStateOf<DeviceState?>(null) }
    var screen by remember { mutableStateOf(Screen.HOME) }
    var now by remember { mutableLongStateOf(System.currentTimeMillis() / 1000) }
    // Requests the person just answered, hidden before the next read says they are gone.
    var answered by remember { mutableStateOf(setOf<String>()) }

    LaunchedEffect(Unit) {
        val result = withContext(Dispatchers.IO) { Native.ensureStarted(context) }
        started = result
        if (result.isFailure) return@LaunchedEffect
        while (true) {
            val read = command(JSONObject().put("op", "state"))
            if (!read.has("error")) {
                val next = DeviceState.parse(read)
                answered = answered.filter { id -> next.approvals.any { it.id == id } }.toSet()
                state = next
            }
            now = System.currentTimeMillis() / 1000
            delay(1000)
        }
    }

    BackHandler(enabled = screen != Screen.HOME) { screen = Screen.HOME }

    val current = state
    val failure = started?.exceptionOrNull()
    when {
        failure != null -> Message("ErisAuth did not start", failure.message ?: "")
        current == null -> Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) { CircularProgressIndicator() }
        !current.joined -> JoinScreen(joining = current.joining, handed = ticket, onHandedTaken = onTicketTaken)
        screen == Screen.ADD_DEVICE -> AddDeviceScreen(
            waiting = current.approvals.map { it.id },
            now = now,
            onBack = { screen = Screen.HOME },
        )
        screen == Screen.PASSPHRASE -> PassphraseScreen(onBack = { screen = Screen.HOME })
        else -> HomeScreen(
            state = current,
            approvals = current.approvals.filter { it.id !in answered },
            now = now,
            onAnswered = { id -> answered = answered + id },
            onAddDevice = { screen = Screen.ADD_DEVICE },
            onPassphrase = { screen = Screen.PASSPHRASE },
        )
    }
}

/** A screen's title row: back, when there is somewhere to go back to, and the title. */
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
private fun Message(title: String, body: String) {
    Column(Modifier.fillMaxSize().padding(24.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Text(title, style = MaterialTheme.typography.titleMedium)
        Text(body, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
}
