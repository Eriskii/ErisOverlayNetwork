package dev.eris.auth.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import dev.eris.auth.PassphraseAction
import dev.eris.auth.passphraseProblem
import kotlinx.coroutines.launch
import org.json.JSONObject

/**
 * The passphrase, which approves what a device may not by itself. Typed with the current one it
 * changes; without, it is set anew, which is how the first one is set and which only a device
 * allowed to reset it may do.
 */
@Composable
fun PassphraseScreen(onBack: () -> Unit) {
    val scope = rememberCoroutineScope()
    var current by remember { mutableStateOf("") }
    var next by remember { mutableStateOf("") }
    var again by remember { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf("") }
    var done by remember { mutableStateOf("") }
    val short = next.isNotEmpty() && next.length < 8
    val mismatch = again.isNotEmpty() && again != next

    fun run(action: PassphraseAction, message: JSONObject, success: String) {
        busy = true
        error = ""
        done = ""
        scope.launch {
            val outcome = command(message)
            busy = false
            error = passphraseProblem(outcome, action)
            if (error.isEmpty()) {
                done = success
                current = ""
                next = ""
                again = ""
            }
        }
    }

    Column(Modifier.fillMaxSize()) {
        SecureWindow()
        Header(title = "Passphrase", onBack = onBack)
        Column(
            Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Text(
                "Type the current passphrase to change it. Leave it empty to set one without it: that is how " +
                    "the first passphrase is set, and only a device allowed to reset it may.",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Secret("Current passphrase", current, enabled = !busy) { current = it }
            Secret("New passphrase", next, enabled = !busy, note = if (short) "At least 8 characters." else null) { next = it }
            Secret("Again", again, enabled = !busy, note = if (mismatch) "The two differ." else null) { again = it }
            if (error.isNotEmpty()) {
                Text(error, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.error)
            }
            if (done.isNotEmpty()) {
                Text(done, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.primary)
            }
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End) {
                Button(
                    onClick = {
                        val message = JSONObject().put("op", "passphrase").put("new", next)
                        if (current.isNotEmpty()) message.put("current", current)
                        run(if (current.isEmpty()) PassphraseAction.RESET else PassphraseAction.CHANGE, message, "The passphrase is set.")
                    },
                    enabled = !busy && next.isNotEmpty() && !short && next == again,
                ) { Text("Save") }
            }
            HorizontalDivider(Modifier.padding(vertical = 8.dp))
            Text(
                "A device that guesses wrong three times waits before it may guess again.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            OutlinedButton(
                onClick = { run(PassphraseAction.UNLOCK, JSONObject().put("op", "unlock"), "Every device may guess again.") },
                enabled = !busy,
            ) { Text("Let every device guess again") }
        }
    }
}

@Composable
private fun Secret(label: String, value: String, enabled: Boolean, note: String? = null, onChange: (String) -> Unit) {
    OutlinedTextField(
        value = value,
        onValueChange = onChange,
        label = { Text(label) },
        singleLine = true,
        enabled = enabled,
        isError = note != null,
        supportingText = note?.let { { Text(it) } },
        visualTransformation = PasswordVisualTransformation(),
        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password),
        modifier = Modifier.fillMaxWidth(),
    )
}
