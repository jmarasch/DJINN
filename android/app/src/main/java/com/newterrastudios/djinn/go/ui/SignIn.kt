package com.newterrastudios.djinn.go.ui

import android.util.Log
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import com.newterrastudios.djinn.go.data.Account
import com.newterrastudios.djinn.go.data.ApiException
import com.newterrastudios.djinn.go.data.GoApi
import com.newterrastudios.djinn.go.data.safeToSignIn
import com.newterrastudios.djinn.go.data.serverAddress
import kotlinx.coroutines.launch
import java.io.IOException

private fun explain(e: IOException): String = when (e) {
    is ApiException -> e.message ?: "The server said no."
    else -> {
        Log.w("DjinnGo", "Couldn't reach the server", e)
        "Couldn't reach the server. Check the address and that this phone is on the right network."
    }
}

@Composable
private fun Form(title: String, note: String?, content: @Composable () -> Unit) {
    Column(
        Modifier
            .fillMaxSize()
            .safeDrawingPadding()
            .imePadding()
            .verticalScroll(rememberScrollState())
            .padding(24.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        Text(title, style = MaterialTheme.typography.headlineMedium)
        if (note != null) {
            Text(note, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
        content()
    }
}

/** Which server: the address from Admin > GO. */
@Composable
fun ConnectScreen(
    account: Account,
    api: GoApi,
    askLocalNetwork: suspend () -> Boolean,
    askCamera: suspend () -> Boolean,
    onConnected: () -> Unit,
) {
    val scope = rememberCoroutineScope()
    var address by rememberSaveable { mutableStateOf(account.server ?: "") }
    var busy by rememberSaveable { mutableStateOf(false) }
    var problem by rememberSaveable { mutableStateOf<String?>(null) }
    var warnAbout by rememberSaveable { mutableStateOf<String?>(null) }
    var scanning by rememberSaveable { mutableStateOf(false) }

    fun connect(server: String) {
        busy = true
        problem = null
        scope.launch {
            val allowed = askLocalNetwork()
            account.setServer(server)
            try {
                api.check()
                onConnected()
            } catch (e: IOException) {
                account.forgetServer()
                problem = explain(e)
                if (!allowed && e !is ApiException) {
                    problem += " A server at home also needs the Nearby devices permission, " +
                        "in this app's settings."
                }
            } finally {
                busy = false
            }
        }
    }

    fun use(server: String?) {
        when {
            server == null -> problem = "That doesn't look like a server address."
            !safeToSignIn(server) -> warnAbout = server
            else -> connect(server)
        }
    }

    if (scanning) {
        ScanScreen(
            onFound = { text ->
                scanning = false
                address = text
                use(serverAddress(text))
            },
            onCancel = { scanning = false },
        )
        return
    }

    Form(
        "DJINN GO",
        "Scan the QR code from GO on a computer (Account > Open on your phone), or enter the address " +
            "Admin > GO shows, like 192.168.1.20:8067. " +
            "Android may ask about nearby devices: that's how it lets an app reach a computer on your home network.",
    ) {
        OutlinedTextField(
            value = address,
            onValueChange = { address = it },
            label = { Text("Server address") },
            singleLine = true,
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri),
            modifier = Modifier.fillMaxWidth(),
        )
        problem?.let { Text(it, color = MaterialTheme.colorScheme.error) }
        Button(
            enabled = !busy && address.isNotBlank(),
            modifier = Modifier.fillMaxWidth(),
            onClick = { use(serverAddress(address)) },
        ) { Text(if (busy) "Connecting..." else "Continue") }
        OutlinedButton(
            enabled = !busy,
            modifier = Modifier.fillMaxWidth(),
            onClick = {
                scope.launch {
                    if (askCamera()) {
                        scanning = true
                    } else {
                        problem = "Scanning needs the camera. You can type the address instead."
                    }
                }
            },
        ) { Text("Scan the QR code") }
    }

    warnAbout?.let { server ->
        AlertDialog(
            onDismissRequest = { warnAbout = null },
            title = { Text("Not encrypted") },
            text = {
                Text(
                    "$server is outside your home network and doesn't use HTTPS. " +
                        "Your password and music would cross the internet where others could read them.",
                )
            },
            confirmButton = {
                TextButton(onClick = {
                    warnAbout = null
                    connect(server)
                }) { Text("Connect anyway") }
            },
            dismissButton = { TextButton(onClick = { warnAbout = null }) { Text("Go back") } },
        )
    }
}

/** Username and password, then the two-step code if the account has one. */
@Composable
fun SignInScreen(account: Account, api: GoApi, onChangeServer: () -> Unit) {
    val scope = rememberCoroutineScope()
    var username by rememberSaveable { mutableStateOf("") }
    var password by rememberSaveable { mutableStateOf("") }
    var ticket by rememberSaveable { mutableStateOf<String?>(null) }
    var code by rememberSaveable { mutableStateOf("") }
    var busy by rememberSaveable { mutableStateOf(false) }
    var problem by rememberSaveable { mutableStateOf<String?>(null) }

    fun submit() {
        busy = true
        problem = null
        scope.launch {
            try {
                val waiting = ticket
                val reply = if (waiting != null) {
                    api.twoStep(waiting, code.trim())
                } else {
                    api.login(username.trim(), password)
                }
                val token = reply.token
                when {
                    token != null -> account.signIn(token)
                    reply.ticket != null -> {
                        ticket = reply.ticket
                        password = ""
                    }
                    else -> problem = "The server's answer made no sense. Is it up to date?"
                }
            } catch (e: IOException) {
                problem = explain(e)
                // A ticket that ran out means starting over.
                if (e is ApiException && ticket != null && "too long" in (e.message ?: "")) ticket = null
            } finally {
                busy = false
            }
        }
    }

    if (ticket != null) {
        Form("Two-step sign-in", "Enter the code from your authenticator app, or a recovery code.") {
            OutlinedTextField(
                value = code,
                onValueChange = { code = it },
                label = { Text("Code") },
                singleLine = true,
                modifier = Modifier.fillMaxWidth(),
            )
            problem?.let { Text(it, color = MaterialTheme.colorScheme.error) }
            Button(
                enabled = !busy && code.isNotBlank(),
                modifier = Modifier.fillMaxWidth(),
                onClick = ::submit,
            ) { Text(if (busy) "Checking..." else "Sign in") }
        }
        return
    }

    Form("Sign in", account.server) {
        OutlinedTextField(
            value = username,
            onValueChange = { username = it },
            label = { Text("Username") },
            singleLine = true,
            modifier = Modifier.fillMaxWidth(),
        )
        OutlinedTextField(
            value = password,
            onValueChange = { password = it },
            label = { Text("Password") },
            singleLine = true,
            visualTransformation = PasswordVisualTransformation(),
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password),
            modifier = Modifier.fillMaxWidth(),
        )
        problem?.let { Text(it, color = MaterialTheme.colorScheme.error) }
        Button(
            enabled = !busy && username.isNotBlank() && password.isNotEmpty(),
            modifier = Modifier.fillMaxWidth(),
            onClick = ::submit,
        ) { Text(if (busy) "Signing in..." else "Sign in") }
        TextButton(onClick = onChangeServer, modifier = Modifier.fillMaxWidth()) {
            Text("Use a different server")
        }
    }
}
