package com.newterrastudios.djinn.go

import com.newterrastudios.djinn.go.data.securityCode
import android.Manifest
import android.app.SearchManager
import android.content.Intent
import android.provider.MediaStore
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
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Modifier
import androidx.core.content.ContextCompat
import androidx.lifecycle.lifecycleScope
import androidx.media3.common.util.UnstableApi
import com.newterrastudios.djinn.go.player.CastState
import com.newterrastudios.djinn.go.player.PlayerState
import com.newterrastudios.djinn.go.player.Spectrum
import com.newterrastudios.djinn.go.work.SmartCacheWorker
import com.newterrastudios.djinn.go.work.SyncWorker
import kotlinx.coroutines.flow.MutableSharedFlow
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
    private val cast by lazy { CastState(this, player) }
    private val spokenRequests = MutableSharedFlow<String>(replay = 1, extraBufferCapacity = 1)

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
        Spectrum.leadMs = app.prefs.visualsLeadMs
        Spectrum.intensity = app.prefs.visualsIntensity
        Spectrum.look = app.prefs.visualLook
        handle(intent)
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
        val server by account.current.collectAsState()
        val scope = rememberCoroutineScope()
        if (server == null) {
            ConnectScreen(
                account,
                api,
                app.offline,
                ::askLocalNetwork,
                askCamera = { ask(Manifest.permission.CAMERA) },
            )
            return
        }
        if (!signedIn) {
            SignInScreen(account, api, onChangeServer = account::leave)
            return
        }
        // Everything below belongs to one account on one server, and starts over on another.
        key(account.scope) {
        val nav = remember {
            Nav(
                api = api,
                player = player,
                offline = app.offline,
                prefs = app.prefs,
                cast = cast,
                server = account.scope.orEmpty(),
                address = account.server.orEmpty(),
                encrypted = account.pin != null || account.server?.startsWith("https://") == true,
                securityCode = account.pin?.let(::securityCode),
                serverName = account.serverName,
                switchServer = {
                    player.stop()
                    account.leave()
                },
                scope = lifecycleScope,
                signOut = {
                    player.stop()
                    scope.launch {
                        // Ends it on the server too, when the server can be reached.
                        try {
                            api.logout()
                        } catch (_: IOException) {
                        }
                        // Otherwise they stay, locked, for whoever signs back in.
                        if (app.prefs.keepRules.signOut != "keep") app.offline.deleteEverything()
                        account.signOut()
                    }
                },
                onOfflineChange = { SyncWorker.enqueue(this@MainActivity) },
                askStorage = {
                    Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q ||
                        ask(Manifest.permission.WRITE_EXTERNAL_STORAGE)
                },
            )
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
            // The admin may have renamed it since.
            api.hello()?.let {
                account.setServerName(it.name)
                nav.serverName = it.name
            }
        }
        LaunchedEffect(Unit) {
            nav.liked.addAll(app.prefs.likedIds)
            try {
                val liked = api.likedIds().tracks
                nav.liked.clear()
                nav.liked.addAll(liked)
                app.prefs.likedIds = liked.toSet()
            } catch (_: IOException) {
                // Hearts stay as they were last seen.
            }
        }
        LaunchedEffect(Unit) {
            // Whether songs may stay on this phone, and anything to send from offline.
            app.offline.checkAccess()
            app.offline.sync()
            nav.canKeepSongs = try {
                api.me().permissions.downloads
            } catch (_: IOException) {
                app.offline.dao.songs(nav.server).isNotEmpty()
            }
            if (nav.canKeepSongs) SmartCacheWorker.schedule(this@MainActivity)
        }
        LaunchedEffect(Unit) {
            try {
                nav.tapAction = api.prefs().tap
            } catch (_: IOException) {
                // A tap plays until the server says otherwise.
            }
        }
        LaunchedEffect(Unit) {
            app.offline.dao.songsFlow(nav.server).collect { songs ->
                nav.keptIds = songs.map { it.id }.toSet()
            }
        }
        LaunchedEffect(Unit) {
            // Replaces kept copies of songs whose file changed.
            SyncWorker.enqueue(this@MainActivity)
        }
        LaunchedEffect(Unit) {
            // "Play ... on DJINN GO" from the assistant.
            spokenRequests.collect { query ->
                val found = try {
                    val results = api.search(query)
                    results.tracks.ifEmpty { results.described?.tracks.orEmpty() }
                } catch (_: IOException) {
                    app.offline.searchPlayable(nav.server, query).map { it.toTrack() }
                }
                if (found.isEmpty()) nav.message = "Nothing found for \"$query\"." else player.play(found)
            }
        }
        LaunchedEffect(Unit) {
            // Collected, not keyed on the value: clearing it mustn't cancel the load.
            snapshotFlow { nav.radioFor }.filterNotNull().collect { seed ->
                nav.radioFor = null
                try {
                    player.queueRadio(seed.id, api.radio(seed.id))
                } catch (_: IOException) {
                    player.error = "Couldn't start radio."
                }
            }
        }
        MainScreen(nav)
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        handle(intent)
    }

    private fun handle(intent: Intent?) {
        if (intent?.action == MediaStore.INTENT_ACTION_MEDIA_PLAY_FROM_SEARCH) {
            intent.getStringExtra(SearchManager.QUERY)?.takeIf { it.isNotBlank() }?.let {
                spokenRequests.tryEmit(it)
            }
        }
    }

    override fun onDestroy() {
        player.disconnect()
        super.onDestroy()
    }
}
