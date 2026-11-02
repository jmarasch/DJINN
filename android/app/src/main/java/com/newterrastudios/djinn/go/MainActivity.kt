package com.newterrastudios.djinn.go

import android.Manifest
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Modifier
import androidx.core.content.ContextCompat
import androidx.media3.common.util.UnstableApi
import com.newterrastudios.djinn.go.player.PlayerState
import com.newterrastudios.djinn.go.ui.ConnectScreen
import com.newterrastudios.djinn.go.ui.DjinnTheme
import com.newterrastudios.djinn.go.ui.MainScreen
import com.newterrastudios.djinn.go.ui.Nav
import com.newterrastudios.djinn.go.ui.SignInScreen
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.flow.filterNotNull
import kotlinx.coroutines.launch
import java.io.IOException

@UnstableApi
class MainActivity : ComponentActivity() {
    private val app get() = application as DjinnApp
    private val player by lazy { PlayerState(app.api) }

    private var asking: CompletableDeferred<Boolean>? = null
    private val askPermission =
        registerForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
            asking?.complete(granted)
        }

    /** Asks if it hasn't been granted, and says whether it is now. */
    private suspend fun ask(permission: String): Boolean {
        if (ContextCompat.checkSelfPermission(this, permission) == PackageManager.PERMISSION_GRANTED) {
            return true
        }
        val answer = CompletableDeferred<Boolean>()
        asking = answer
        askPermission.launch(permission)
        return answer.await()
    }

    /** Newer Android blocks the home network without this, even for plain sockets. */
    private suspend fun askLocalNetwork(): Boolean =
        Build.VERSION.SDK_INT < Build.VERSION_CODES.BAKLAVA ||
            ask(Manifest.permission.ACCESS_LOCAL_NETWORK)

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        setContent {
            DjinnTheme {
                Surface(Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.background) {
                    Root()
                }
            }
        }
    }

    @androidx.compose.runtime.Composable
    private fun Root() {
        val account = app.account
        val api = app.api
        val signedIn by account.signedIn.collectAsState()
        var connected by rememberSaveable { mutableStateOf(account.server != null) }
        val scope = rememberCoroutineScope()
        if (!signedIn) {
            if (connected) {
                SignInScreen(account, api, onChangeServer = { connected = false })
            } else {
                ConnectScreen(account, api, ::askLocalNetwork, onConnected = { connected = true })
            }
            return
        }
        val nav = remember {
            Nav(api, player, signOut = {
                player.stop()
                scope.launch {
                    // Ends it on the server too, when the server can be reached.
                    try {
                        api.logout()
                    } catch (_: IOException) {
                    }
                    account.signOut()
                }
            })
        }
        LaunchedEffect(Unit) {
            player.connect(this@MainActivity)
            askLocalNetwork()
            // The music plays either way; this only decides the notification.
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                ask(Manifest.permission.POST_NOTIFICATIONS)
            }
        }
        LaunchedEffect(Unit) { player.followPosition() }
        LaunchedEffect(Unit) {
            try {
                nav.liked.addAll(api.likedIds().tracks)
            } catch (_: IOException) {
                // Hearts just start empty.
            }
        }
        LaunchedEffect(Unit) {
            // Collected, not keyed on the value: clearing it mustn't cancel the load.
            snapshotFlow { nav.radioFor }.filterNotNull().collect { seed ->
                nav.radioFor = null
                try {
                    player.queueRadio(api.radio(seed.id))
                } catch (_: IOException) {
                    player.error = "Couldn't start radio."
                }
            }
        }
        MainScreen(nav)
    }

    override fun onDestroy() {
        player.disconnect()
        super.onDestroy()
    }
}
