package com.newterrastudios.djinn.go.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Home
import androidx.compose.material.icons.filled.LibraryMusic
import androidx.compose.material.icons.filled.Pause
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.filled.Search
import androidx.compose.material.icons.filled.SkipNext
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Snackbar
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.media3.common.util.UnstableApi
import com.newterrastudios.djinn.go.data.GoApi
import com.newterrastudios.djinn.go.data.OfflineStore
import com.newterrastudios.djinn.go.data.Prefs
import com.newterrastudios.djinn.go.player.CastState
import com.newterrastudios.djinn.go.data.Track
import com.newterrastudios.djinn.go.player.PlayerState
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.delay

sealed interface Screen {
    data class Album(val id: Long) : Screen
    data class Artist(val id: Long) : Screen
    data class Playlist(val id: Long) : Screen
    data object NowPlaying : Screen
    data object Settings : Screen
    data object Visuals : Screen
}

enum class Tab(val label: String) { Home("Home"), Library("Library"), Search("Search") }

/** Where you are in the app, and what every screen needs. */
@UnstableApi
class Nav(
    val api: GoApi,
    val player: PlayerState,
    val offline: OfflineStore,
    val prefs: Prefs,
    val cast: CastState,
    val server: String,
    val signOut: () -> Unit,
    /** For work that must outlive the screen or menu that started it. */
    val scope: CoroutineScope,
    /** Asks for what saving a system sound needs on older Android. */
    val askStorage: suspend () -> Boolean,
    private val onOfflineChange: () -> Unit,
) {
    var tab by mutableStateOf(Tab.Home)
    /** The role lets this account keep songs on the phone. */
    var canKeepSongs by mutableStateOf(false)
    /** Songs on this phone, for the mark beside them. */
    var keptIds by mutableStateOf<Set<Long>>(emptySet())
    /** Something to say briefly. */
    var message by mutableStateOf<String?>(null)
    val stack = mutableStateListOf<Screen>()
    var menuFor by mutableStateOf<Track?>(null)
    val liked = mutableStateListOf<Long>()
    /** A song to start radio from. */
    var radioFor by mutableStateOf<Track?>(null)

    fun syncLater() = onOfflineChange()

    fun push(screen: Screen) {
        stack.add(screen)
    }

    fun pop() {
        stack.removeLastOrNull()
    }

    /** Leaves Now Playing for another screen. */
    fun goTo(screen: Screen) {
        stack.remove(Screen.NowPlaying)
        stack.add(screen)
    }
}

@UnstableApi
@Composable
fun MainScreen(nav: Nav) {
    BackHandler(enabled = nav.stack.isNotEmpty()) { nav.pop() }
    val top = nav.stack.lastOrNull()
    if (top == Screen.Visuals) {
        FullVisuals(nav)
        return
    }
    if (top == Screen.NowPlaying) {
        NowPlayingScreen(nav)
        nav.menuFor?.let { SongMenu(it, nav) }
        return
    }
    Scaffold(
        bottomBar = {
            Column {
                MiniPlayer(nav)
                NavigationBar {
                    Tab.entries.forEach { tab ->
                        NavigationBarItem(
                            selected = nav.tab == tab && top == null,
                            onClick = {
                                nav.stack.clear()
                                nav.tab = tab
                            },
                            icon = {
                                Icon(
                                    when (tab) {
                                        Tab.Home -> Icons.Default.Home
                                        Tab.Library -> Icons.Default.LibraryMusic
                                        Tab.Search -> Icons.Default.Search
                                    },
                                    contentDescription = null,
                                )
                            },
                            label = { Text(tab.label) },
                        )
                    }
                }
            }
        },
    ) { insets ->
        Box(
            Modifier
                .fillMaxSize()
                .padding(bottom = insets.calculateBottomPadding())
                .statusBarsPadding(),
        ) {
            when (top) {
                is Screen.Album -> AlbumScreen(top.id, nav)
                is Screen.Artist -> ArtistScreen(top.id, nav)
                is Screen.Playlist -> PlaylistScreen(top.id, nav)
                Screen.Settings -> SettingsScreen(nav)
                else -> when (nav.tab) {
                    Tab.Home -> HomeScreen(nav)
                    Tab.Library -> LibraryScreen(nav)
                    Tab.Search -> SearchScreen(nav)
                }
            }
            LaunchedEffect(nav.message) {
                // Errors stay until read; other messages clear themselves.
                if (nav.message != null) {
                    delay(4000)
                    nav.message = null
                }
            }
            (nav.player.error ?: nav.message)?.let { message ->
                Snackbar(
                    Modifier
                        .align(Alignment.BottomCenter)
                        .padding(12.dp)
                        .clickable {
                            nav.player.error = null
                            nav.message = null
                        },
                ) { Text(message) }
            }
        }
    }
    nav.menuFor?.let { SongMenu(it, nav) }
}

/** The song playing, along the bottom. Opens Now Playing. */
@UnstableApi
@Composable
private fun MiniPlayer(nav: Nav) {
    val player = nav.player
    val item = player.current ?: return
    Column(Modifier.background(MaterialTheme.colorScheme.surfaceContainerHigh)) {
        if (player.durationMs > 0) {
            LinearProgressIndicator(
                progress = { (player.positionMs.toFloat() / player.durationMs).coerceIn(0f, 1f) },
                modifier = Modifier.fillMaxWidth(),
            )
        }
        Row(
            Modifier
                .fillMaxWidth()
                .clickable { nav.push(Screen.NowPlaying) }
                .padding(horizontal = 12.dp, vertical = 6.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Art(item.mediaMetadata.artworkUri?.toString(), 44.dp)
            Column(Modifier.weight(1f)) {
                Text(item.mediaMetadata.title?.toString() ?: "", maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(
                    item.mediaMetadata.artist?.toString() ?: "",
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            IconButton(onClick = player::toggle) {
                Icon(
                    if (player.playing) Icons.Default.Pause else Icons.Default.PlayArrow,
                    contentDescription = if (player.playing) "Pause" else "Play",
                )
            }
            IconButton(onClick = player::next, enabled = player.upNext.isNotEmpty()) {
                Icon(Icons.Default.SkipNext, contentDescription = "Next")
            }
        }
    }
}
