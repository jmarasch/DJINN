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
import androidx.compose.foundation.text.KeyboardActions
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
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import com.newterrastudios.djinn.go.data.Account
import com.newterrastudios.djinn.go.data.ApiException
import com.newterrastudios.djinn.go.data.GoApi
import com.newterrastudios.djinn.go.data.safeToSignIn
import com.newterrastudios.djinn.go.data.serverAddress
import com.newterrastudios.djinn.go.i18n.t
import kotlinx.coroutines.launch
import java.io.IOException

private fun explain(e: IOException): String = when (e) {
    is ApiException -> e.message ?: t("signin.server_said_no")
    else -> {
        Log.w("DjinnGo", "Couldn't reach the server", e)
        t("signin.unreachable")
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
                        if (fromQr) t("signin.not_qr_server") else t("signin.not_announced_server")
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
            problem = t("signin.bad_address")
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
        t("signin.nearby_note"),
    ) {
        if (saved.isNotEmpty()) {
            Text(t("signin.your_servers"), style = MaterialTheme.typography.titleMedium)
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
                            Icon(Icons.Default.Close, t("signin.remove_named", "name" to (server.name ?: server.address)))
                        }
                    }
                }
            }
        }
        Text(t("signin.on_this_network"), style = MaterialTheme.typography.titleMedium)
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
                t("signin.recognize_note"),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            lookedLong -> Text(
                t("signin.none_heard"),
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            else -> Row(
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                CircularProgressIndicator(Modifier.size(20.dp), strokeWidth = 2.dp)
                Text(t("signin.looking"), color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
        }
        Text(t("signin.connect_yourself"), style = MaterialTheme.typography.titleMedium, modifier = Modifier.padding(top = 8.dp))
        Text(
            t("signin.connect_help"),
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        OutlinedTextField(
            value = address,
            onValueChange = { address = it },
            label = { Text(t("signin.server_address")) },
            singleLine = true,
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri, imeAction = ImeAction.Go),
            keyboardActions = KeyboardActions(onGo = { if (!busy && address.isNotBlank()) use(serverAddress(address)) }),
            modifier = Modifier.fillMaxWidth(),
        )
        problem?.let { Text(it, color = MaterialTheme.colorScheme.error) }
        Button(
            enabled = !busy && address.isNotBlank(),
            modifier = Modifier.fillMaxWidth(),
            onClick = { use(serverAddress(address)) },
        ) { Text(if (busy) t("signin.connecting") else t("signin.continue")) }
        OutlinedButton(
            enabled = !busy,
            modifier = Modifier.fillMaxWidth(),
            onClick = {
                scope.launch {
                    if (askCamera()) {
                        scanning = true
                    } else {
                        problem = t("signin.camera_needed")
                    }
                }
            },
        ) { Text(t("signin.scan_qr")) }
    }

    unencrypted?.let { probed ->
        AlertDialog(
            onDismissRequest = { unencrypted = null },
            title = { Text(t("signin.not_encrypted")) },
            text = {
                Text(
                    if (safeToSignIn(probed.address)) {
                        t("signin.unencrypted_home")
                    } else {
                        t("signin.unencrypted_away", "address" to probed.address)
                    },
                )
            },
            confirmButton = {
                TextButton(onClick = {
                    unencrypted = null
                    accept(probed, account.saved(probed.hello?.id, probed.address))
                }) { Text(t("signin.connect_anyway")) }
            },
            dismissButton = { TextButton(onClick = { unencrypted = null }) { Text(t("signin.go_back")) } },
        )
    }

    changed?.let { (before, probed) ->
        AlertDialog(
            onDismissRequest = { changed = null },
            title = { Text(t("signin.cert_changed")) },
            text = {
                Text(
                    t(
                        "signin.cert_changed_body",
                        "name" to (before.name ?: before.address),
                        "code" to (probed.pin?.let(::securityCode) ?: t("signin.no_code")),
                    ),
                )
            },
            confirmButton = {
                TextButton(onClick = {
                    changed = null
                    // A password is needed again, so it only goes to the server just checked.
                    accept(probed, before.copy(token = null, accountId = null))
                }) { Text(t("signin.trust_it")) }
            },
            dismissButton = { TextButton(onClick = { changed = null }) { Text(t("signin.go_back")) } },
        )
    }

    removing?.let { server ->
        AlertDialog(
            onDismissRequest = { removing = null },
            title = { Text(t("signin.remove_question", "name" to (server.name ?: t("signin.this_server")))) },
            text = { Text(t("signin.remove_body")) },
            confirmButton = {
                TextButton(onClick = {
                    removing = null
                    scope.launch {
                        offline.deleteForServer(server)
                        account.forget(server)
                    }
                }) { Text(t("signin.remove_and_delete")) }
            },
            dismissButton = { TextButton(onClick = { removing = null }) { Text(t("signin.keep_it")) } },
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
                    else -> problem = t("signin.nonsense")
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
        Form(t("signin.two_step"), t("signin.two_step_help")) {
            OutlinedTextField(
                value = code,
                onValueChange = { code = it },
                label = { Text(t("signin.code")) },
                singleLine = true,
                modifier = Modifier.fillMaxWidth(),
            )
            problem?.let { Text(it, color = MaterialTheme.colorScheme.error) }
            Button(
                enabled = !busy && code.isNotBlank(),
                modifier = Modifier.fillMaxWidth(),
                onClick = ::submit,
            ) { Text(if (busy) t("signin.checking") else t("signin.sign_in")) }
        }
        return
    }

    Form(
        account.serverName?.let { t("signin.sign_in_to", "name" to it) } ?: t("signin.sign_in"),
        account.server + "\n" + when {
            account.pin != null -> t("signin.encrypted_code", "code" to (account.pin?.let(::securityCode) ?: ""))
            account.server?.startsWith("https://") == true -> t("signin.encrypted")
            else -> t("signin.not_encrypted_note")
        },
    ) {
        OutlinedTextField(
            value = username,
            onValueChange = { username = it },
            label = { Text(t("signin.username")) },
            singleLine = true,
            modifier = Modifier.fillMaxWidth(),
        )
        OutlinedTextField(
            value = password,
            onValueChange = { password = it },
            label = { Text(t("signin.password")) },
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
        ) { Text(if (busy) t("signin.signing_in") else t("signin.sign_in")) }
        TextButton(onClick = onChangeServer, modifier = Modifier.fillMaxWidth()) {
            Text(t("signin.different_server"))
        }
    }
}
