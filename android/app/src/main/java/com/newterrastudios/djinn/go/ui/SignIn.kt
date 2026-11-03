package com.newterrastudios.djinn.go.ui

import android.util.Log
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.size
import androidx.compose.material3.Card
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.platform.LocalContext
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Close
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import com.newterrastudios.djinn.go.data.FoundServer
import com.newterrastudios.djinn.go.data.OfflineStore
import com.newterrastudios.djinn.go.data.Probed
import com.newterrastudios.djinn.go.data.SavedServer
import com.newterrastudios.djinn.go.data.ServerFinder
import com.newterrastudios.djinn.go.data.pinFromQr
import com.newterrastudios.djinn.go.data.securityCode
import kotlinx.coroutines.delay
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

/** How long to look before saying nothing's been heard. */
private const val LOOKING_MS = 8_000L

/** Which server: one heard on this network, a QR code, or the address from Admin > GO. */
@Composable
fun ConnectScreen(
    account: Account,
    api: GoApi,
    offline: OfflineStore,
    askLocalNetwork: suspend () -> Boolean,
    askCamera: suspend () -> Boolean,
) {
    val scope = rememberCoroutineScope()
    var address by rememberSaveable { mutableStateOf("") }
    var busy by rememberSaveable { mutableStateOf(false) }
    var problem by rememberSaveable { mutableStateOf<String?>(null) }
    /** Reached, but only without encryption. */
    var unencrypted by remember { mutableStateOf<Probed?>(null) }
    /** A known server presenting a different certificate. */
    var changed by remember { mutableStateOf<Pair<SavedServer, Probed>?>(null) }
    var removing by remember { mutableStateOf<SavedServer?>(null) }
    val saved by account.servers.collectAsState()
    var scanning by rememberSaveable { mutableStateOf(false) }
    var lookedLong by rememberSaveable { mutableStateOf(false) }
    val context = LocalContext.current
    val finder = remember { ServerFinder(context) }
    val heard by finder.servers.collectAsState()
    // A known server may have moved; its certificate proves it's the same one.
    LaunchedEffect(heard) { heard.forEach { account.moved(it.id, it.address) } }
    val found = heard.filter { server -> saved.none { it.id == server.id } }
    LaunchedEffect(Unit) {
        // Newer Android only lets an app hear the network with permission.
        askLocalNetwork()
        finder.start()
        delay(LOOKING_MS)
        lookedLong = true
    }
    DisposableEffect(Unit) { onDispose { finder.stop() } }

    fun accept(probed: Probed, before: SavedServer?) {
        val sameServer = before?.id == null || before.id == probed.hello?.id
        account.use(
            SavedServer(
                address = probed.address,
                id = probed.hello?.id ?: before?.id,
                name = probed.hello?.name ?: before?.name,
                // A sign-in only goes back to the server it came from.
                token = before?.token.takeIf { sameServer },
                accountId = before?.accountId.takeIf { sameServer },
                pin = probed.pin,
            ),
        )
    }

    /** `expected` is the certificate a QR code or announcement said to look for. */
    fun connect(server: String, expected: String?, fromQr: Boolean) {
        busy = true
        problem = null
        scope.launch {
            val allowed = askLocalNetwork()
            try {
                val probed = api.probe(server)
                val before = account.saved(probed.hello?.id, server)
                when {
                    expected != null && probed.pin != expected -> problem =
                        (if (fromQr) "That isn't the server the QR code is for" else "That server isn't what it announced") +
                            ": its certificate is different. Someone on this network may be in the way."
                    before?.pin != null && probed.pin != before.pin -> changed = before to probed
                    !probed.encrypted -> unencrypted = probed
                    else -> accept(probed, before)
                }
            } catch (e: IOException) {
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

    fun use(server: String?, expected: String? = null, fromQr: Boolean = false) {
        if (server == null) {
            problem = "That doesn't look like a server address."
        } else {
            connect(server, expected, fromQr)
        }
    }

    if (scanning) {
        ScanScreen(
            onFound = { text ->
                scanning = false
                address = serverAddress(text) ?: text
                use(serverAddress(text), pinFromQr(text), fromQr = true)
            },
            onCancel = { scanning = false },
        )
        return
    }

    Form(
        "DJINN GO",
        "Android may ask about nearby devices: that's how it lets an app find and reach a computer on your network.",
    ) {
        if (saved.isNotEmpty()) {
            Text("Your servers", style = MaterialTheme.typography.titleMedium)
            saved.forEach { server ->
                Card(
                    Modifier
                        .fillMaxWidth()
                        .clickable(enabled = !busy) { account.use(server) },
                ) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Column(
                            Modifier
                                .weight(1f)
                                .padding(16.dp),
                        ) {
                            Text(server.name ?: server.address, style = MaterialTheme.typography.titleMedium)
                            Text(
                                server.address + if (server.token != null) "" else " (sign in needed)",
                                style = MaterialTheme.typography.bodySmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                        IconButton(onClick = { removing = server }) {
                            Icon(Icons.Default.Close, "Remove ${server.name ?: server.address}")
                        }
                    }
                }
            }
        }
        Text("On this network", style = MaterialTheme.typography.titleMedium)
        found.forEach { server: FoundServer ->
            Card(
                Modifier
                    .fillMaxWidth()
                    .clickable(enabled = !busy) { use(server.address, server.pin) },
            ) {
                Column(Modifier.padding(16.dp)) {
                    Text(server.name, style = MaterialTheme.typography.titleMedium)
                    Text(
                        server.address,
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
        }
        when {
            found.isNotEmpty() -> Text(
                "Anyone on a network can give a computer any name, so only sign in to a server you recognize.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            lookedLong -> Text(
                "No servers heard yet. GO has to be on, on a computer on this network, and some networks block this. " +
                    "You can still scan or enter the address below.",
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            else -> Row(
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                CircularProgressIndicator(Modifier.size(20.dp), strokeWidth = 2.dp)
                Text("Looking for servers...", color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
        }
        Text("Or connect yourself", style = MaterialTheme.typography.titleMedium, modifier = Modifier.padding(top = 8.dp))
        Text(
            "Scan the QR code from GO on a computer (Account > Open on your phone), or enter the address " +
                "Admin > GO shows, like 192.168.1.20:8067.",
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
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

    unencrypted?.let { probed ->
        AlertDialog(
            onDismissRequest = { unencrypted = null },
            title = { Text("Not encrypted") },
            text = {
                Text(
                    if (safeToSignIn(probed.address)) {
                        "This server isn't offering encryption, so others on this network could see your password " +
                            "and what you play. Its admin can turn it on under Admin > GO."
                    } else {
                        "${probed.address} is outside your home network and isn't offering encryption. " +
                            "Your password and music would cross the internet where others could read them."
                    },
                )
            },
            confirmButton = {
                TextButton(onClick = {
                    unencrypted = null
                    accept(probed, account.saved(probed.hello?.id, probed.address))
                }) { Text("Connect anyway") }
            },
            dismissButton = { TextButton(onClick = { unencrypted = null }) { Text("Go back") } },
        )
    }

    changed?.let { (before, probed) ->
        AlertDialog(
            onDismissRequest = { changed = null },
            title = { Text("This server's certificate changed") },
            text = {
                Text(
                    "${before.name ?: before.address} is presenting a different certificate from last time. " +
                        "That happens if it was set up again, but it's also what someone in the middle would look like. " +
                        "Check that Admin > GO shows this code: " + (probed.pin?.let(::securityCode) ?: "none, it's not encrypted"),
                )
            },
            confirmButton = {
                TextButton(onClick = {
                    changed = null
                    // A password is needed again, so it only goes to the server just checked.
                    accept(probed, before.copy(token = null, accountId = null))
                }) { Text("It matches: trust it") }
            },
            dismissButton = { TextButton(onClick = { changed = null }) { Text("Go back") } },
        )
    }

    removing?.let { server ->
        AlertDialog(
            onDismissRequest = { removing = null },
            title = { Text("Remove ${server.name ?: "this server"}?") },
            text = { Text("This forgets the server and deletes the songs and system sounds kept on this phone for it.") },
            confirmButton = {
                TextButton(onClick = {
                    removing = null
                    scope.launch {
                        offline.deleteForServer(server)
                        account.forget(server)
                    }
                }) { Text("Remove and delete") }
            },
            dismissButton = { TextButton(onClick = { removing = null }) { Text("Keep it") } },
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
                    token != null -> {
                        // Songs and settings are per account, so find out whose this is first.
                        account.holdToken(token)
                        try {
                            account.signIn(token, api.me().id)
                        } finally {
                            account.holdToken(null)
                        }
                    }
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

    Form(
        account.serverName?.let { "Sign in to $it" } ?: "Sign in",
        account.server + "\n" + when {
            account.pin != null -> "Encrypted. Security code ${account.pin?.let(::securityCode)}, which Admin > GO shows too."
            account.server?.startsWith("https://") == true -> "Encrypted."
            else -> "Not encrypted: others on this network could see your password."
        },
    ) {
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
