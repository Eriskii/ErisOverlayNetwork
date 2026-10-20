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
import androidx.compose.material3.MaterialTheme
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
import dev.eris.auth.newPassphraseProblem
import dev.eris.auth.passphraseProblem
import kotlinx.coroutines.launch

/**
 * The ErisDB passphrase, which approving an app for ErisDB takes and which ErisDB itself checks.
 * The person changes it here, typing the current one; the first one, and resets, are set where
 * ErisDB runs.
 */
@Composable
fun PassphraseScreen(onBack: () -> Unit) {
    val scope = rememberCoroutineScope()
    var current by remember { mutableStateOf("") }
    var next by remember { mutableStateOf("") }
    var again by remember { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    var done by remember { mutableStateOf(false) }
    val typed = newPassphraseProblem(next, again)

    Column(Modifier.fillMaxSize()) {
        SecureWindow()
        Header(title = "ErisDB passphrase", onBack = onBack)
        Column(
            Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Text(
                "Approving an app for ErisDB takes this passphrase, which ErisDB checks. A phone that guesses " +
                    "wrong too often waits before it may guess again. The first passphrase is set where ErisDB runs.",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Secret("Current passphrase", current, enabled = !busy) { current = it }
            Secret("New passphrase", next, enabled = !busy) { next = it }
            Secret("Again", again, enabled = !busy) { again = it }
            Problem(typed ?: error)
            if (done) {
                Text("The passphrase is changed.", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.primary)
            }
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End) {
                Button(
                    onClick = {
                        busy = true
                        error = null
                        done = false
                        scope.launch {
                            val outcome = command(op("passphrase").put("current", current).put("new", next))
                            busy = false
                            error = passphraseProblem(outcome)
                            if (error == null) {
                                done = true
                                current = ""
                                next = ""
                                again = ""
                            }
                        }
                    },
                    enabled = !busy && current.isNotEmpty() && next.isNotEmpty() && next == again && typed == null,
                ) { Text("Change") }
            }
        }
    }
}

@Composable
private fun Secret(label: String, value: String, enabled: Boolean, onChange: (String) -> Unit) {
    OutlinedTextField(
        value = value,
        onValueChange = onChange,
        label = { Text(label) },
        singleLine = true,
        enabled = enabled,
        visualTransformation = PasswordVisualTransformation(),
        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password),
        modifier = Modifier.fillMaxWidth(),
    )
}
