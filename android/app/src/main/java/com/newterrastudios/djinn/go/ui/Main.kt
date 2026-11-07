package com.newterrastudios.djinn.go.ui

import com.newterrastudios.djinn.go.data.AppRelease
import androidx.compose.material3.TextButton
import com.newterrastudios.djinn.go.data.AppUpdater
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
import androidx.compose.material.icons.filled.PlayCircle
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
    data class Genre(val name: String) : Screen
    data object NowPlaying : Screen
    data object Settings : Screen
    data object Visuals : Screen
    data object Cover : Screen
    data object Year : Screen
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
    /** Whose songs and per-server settings: one account on one server. */
    val server: String,
    val address: String,
    val encrypted: Boolean,
    val securityCode: String?,
    val updater: AppUpdater,
    serverName: String?,
    /** Back to choosing a server, still signed in to this one. */
    val switchServer: () -> Unit,
    val signOut: () -> Unit,
    /** For work that must outlive the screen or menu that started it. */
    val scope: CoroutineScope,
    /** Asks for what saving a system sound needs on older Android. */
    val askStorage: suspend () -> Boolean,
    private val onOfflineChange: () -> Unit,
) {
    var tab by mutableStateOf(Tab.Home)
    // How the library's lists are ordered, until the app closes.
    var artistSort by mutableStateOf(ARTIST_SORTS.first().first)
    var albumSort by mutableStateOf(ALBUM_SORTS.first().first)
    var albumFilter by mutableStateOf(ALBUM_FILTERS.first().first)
    var songSort by mutableStateOf(SONG_SORTS.first().first)
    var keptView by mutableStateOf(KEPT_VIEWS.first().first)
    /** A newer version of this app that the server carries. */
    var update by mutableStateOf<AppRelease?>(null)
    /** What the server calls itself. */
    var serverName by mutableStateOf(serverName)
    /** Styles this device can't keep up with. */
    var slowStyles by mutableStateOf(prefs.visualsTooSlow)
    /** The styles to offer here. Shapes are never slow, so it's never empty. */
    val visualStyles get() = VisualStyle.moving.filter { it.name !in slowStyles }
    /** The role lets this account keep songs on the phone. */
    var canKeepSongs by mutableStateOf(false)
    /** The role lets this account share its playlists. */
    var canShare by mutableStateOf(false)
    /** What a tap on a song does: "play", "queue" (next) or "end". */
    var tapAction by mutableStateOf("play")
    /** Songs on this phone, for the mark beside them. */
    var keptIds by mutableStateOf<Set<Long>>(emptySet())
    /** Something to say briefly. */
    var message by mutableStateOf<String?>(null)
    /** Where you are in the genre cloud, kept while a genre's page is open. */
    var genrePath by mutableStateOf(emptyList<String>())
    /** Takes back what the message is about, while it shows. */
    var undo by mutableStateOf<(() -> Unit)?>(null)
    val stack = mutableStateListOf<Screen>()
    var menuFor by mutableStateOf<Track?>(null)
    val liked = mutableStateListOf<Long>()
    val likedAlbums = mutableStateListOf<Long>()
    val likedArtists = mutableStateListOf<Long>()
    /** Library lists show only what you like. */
    var favoritesOnly by mutableStateOf(false)
    /** The update you've been asked about since opening the app. */
    var askedAboutUpdate by mutableStateOf(false)
    /** A song to start radio from. */
    var radioFor by mutableStateOf<Track?>(null)

    fun syncLater() = onOfflineChange()

    fun push(screen: Screen) {
        stack.add(screen)
    }

    fun pop() {
        stack.removeLastOrNull()
    }

    /** The style last chosen, or a good one when that's the cover or too slow here. */
    fun currentStyle(): VisualStyle {
        val styles = visualStyles
        val chosen = VisualStyle.at(prefs.visuals)
        return when {
            chosen in styles -> chosen
            VisualStyle.Warp in styles -> VisualStyle.Warp
            else -> styles.first()
        }
    }

    fun styleTooSlow(style: VisualStyle) {
        slowStyles = slowStyles + style.name
        prefs.visualsTooSlow = slowStyles
        message = "${style.label} is too much for this device, so it's been left out. Settings can try it again."
    }

    fun retrySlowStyles() {
        slowStyles = emptySet()
        prefs.visualsTooSlow = emptySet()
    }

    /** Pulls up the player, from anywhere. */
    fun showPlayer() {
        stack.remove(Screen.NowPlaying)
        stack.add(Screen.NowPlaying)
    }

    /** Leaves Now Playing for another screen. */
    fun goTo(screen: Screen) {
        stack.remove(Screen.NowPlaying)
        // Already what's underneath, so Back shouldn't show it twice.
        if (stack.lastOrNull() != screen) stack.add(screen)
    }
}

@UnstableApi
@Composable
fun MainScreen(nav: Nav) {
    BackHandler(enabled = nav.stack.isNotEmpty()) { nav.pop() }
    val top = nav.stack.lastOrNull()
    if (top == Screen.Visuals || top == Screen.Cover) {
        FullScreen(nav, cover = top == Screen.Cover)
        return
    }
    Scaffold(
        bottomBar = {
            Column {
                if (top != Screen.NowPlaying) MiniPlayer(nav)
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
                    NavigationBarItem(
                        selected = top == Screen.NowPlaying,
                        enabled = nav.player.current != null,
                        onClick = nav::showPlayer,
                        icon = { Icon(Icons.Default.PlayCircle, contentDescription = null) },
                        label = { Text("Playing") },
                    )
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
                is Screen.Genre -> GenreScreen(top.name, nav)
                Screen.Settings -> SettingsScreen(nav)
                Screen.Year -> YearScreen(nav)
                Screen.NowPlaying -> NowPlayingScreen(nav)
                else -> when (nav.tab) {
                    Tab.Home -> HomeScreen(nav)
                    Tab.Library -> LibraryScreen(nav)
                    Tab.Search -> SearchScreen(nav)
                }
            }
            LaunchedEffect(nav.message) {
                // Errors stay until read; other messages clear themselves.
                if (nav.message != null) {
                    delay(if (nav.undo != null) 6000 else 4000)
                    nav.message = null
                    nav.undo = null
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
                            nav.undo = null
                        },
                    action = nav.undo?.let { undo ->
                        {
                            TextButton(onClick = {
                                undo()
                                nav.message = null
                                nav.undo = null
                            }) { Text("Undo") }
                        }
                    },
                ) { Text(message) }
            }
        }
    }
    nav.menuFor?.let { SongMenu(it, nav) }
    UpdatePrompt(nav)
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
                .clickable(onClick = nav::showPlayer)
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
