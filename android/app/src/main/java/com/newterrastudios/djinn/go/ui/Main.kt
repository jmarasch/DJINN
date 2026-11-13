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
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material.icons.filled.Favorite
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
import androidx.compose.material3.NavigationBarItemDefaults
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import java.io.IOException
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.ui.platform.LocalConfiguration
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
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.unit.dp
import androidx.media3.common.util.UnstableApi
import com.newterrastudios.djinn.go.data.GoApi
import com.newterrastudios.djinn.go.data.OfflineStore
import com.newterrastudios.djinn.go.data.Prefs
import com.newterrastudios.djinn.go.player.CastState
import com.newterrastudios.djinn.go.data.Track
import com.newterrastudios.djinn.go.player.PlayerState
import com.newterrastudios.djinn.go.i18n.t
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.delay

sealed interface Screen {
    data class Album(val id: Long) : Screen
    data class Artist(val id: Long) : Screen
    /** Every song by an artist. */
    data class ArtistSongs(val id: Long) : Screen
    data class Playlist(val id: Long) : Screen
    data class Genre(val name: String) : Screen
    data object NowPlaying : Screen
    data object Settings : Screen
    data object Visuals : Screen
    data object Cover : Screen
    data object Year : Screen
}

/** The tabs, with their names' translation keys. */
enum class Tab(val key: String) { Home("go.home"), Library("nav.library"), Mine("go.your_music"), Account("go.account") }

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
    /** Library lists as picture tiles rather than rows. */
    var libraryGrid by mutableStateOf(prefs.libraryGrid)
    /** The house's background picture. */
    var appearance by mutableStateOf(com.newterrastudios.djinn.go.data.Appearance())
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
    // The house's own background picture, over the page's backdrop.
    LaunchedEffect(Unit) {
        try {
            nav.appearance = nav.api.appearance()
        } catch (_: IOException) {
            // Keep the default picture.
        }
    }
    HouseBackground(nav)
    Scaffold(
        // The page's backdrop shows through.
        containerColor = Color.Transparent,
        bottomBar = {
            Column {
                if (top != Screen.NowPlaying) MiniPlayer(nav)
                // Icons only on phones; tablets (600 dp and wider) keep the words.
                val labels = LocalConfiguration.current.screenWidthDp >= 600
                NavigationBar(
                    containerColor = MaterialTheme.colorScheme.background.copy(alpha = 0.88f),
                    modifier = Modifier.neonTopEdge(),
                ) {
                    // Playing first, left of Home.
                    NavigationBarItem(
                        selected = top == Screen.NowPlaying,
                        onClick = nav::showPlayer,
                        colors = neonItemColors(),
                        icon = {
                            Icon(
                                Icons.Default.PlayCircle,
                                contentDescription = if (labels) null else t("nav.playing"),
                            )
                        },
                        label = if (labels) {
                            { Text(t("nav.playing")) }
                        } else {
                            null
                        },
                    )
                    Tab.entries.forEach { tab ->
                        NavigationBarItem(
                            selected = nav.tab == tab && top == null,
                            onClick = {
                                nav.stack.clear()
                                nav.tab = tab
                            },
                            colors = neonItemColors(),
                            icon = {
                                Icon(
                                    when (tab) {
                                        Tab.Home -> Icons.Default.Home
                                        Tab.Library -> Icons.Default.Search
                                        Tab.Mine -> Icons.Default.Favorite
                                        Tab.Account -> Icons.Default.Settings
                                    },
                                    contentDescription = if (labels) null else t(tab.key),
                                )
                            },
                            label = if (labels) {
                                { Text(t(tab.key)) }
                            } else {
                                null
                            },
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
                is Screen.ArtistSongs -> ArtistSongsScreen(top.id, nav)
                is Screen.Playlist -> PlaylistScreen(top.id, nav)
                is Screen.Genre -> GenreScreen(top.name, nav)
                Screen.Settings -> SettingsScreen(nav)
                Screen.Year -> YearScreen(nav)
                Screen.NowPlaying -> NowPlayingScreen(nav)
                else -> when (nav.tab) {
                    Tab.Home -> HomeScreen(nav)
                    Tab.Library -> SearchScreen(nav)
                    Tab.Mine -> YourMusicScreen(nav)
                    Tab.Account -> SettingsScreen(nav)
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
                            }) { Text(t("common.undo")) }
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
    // A neon card floating over the page's bottom edge.
    Column(
        Modifier
            .padding(horizontal = 8.dp, vertical = 6.dp)
            .neon(RoundedCornerShape(16.dp)),
    ) {
        if (player.durationMs > 0) {
            LinearProgressIndicator(
                progress = { (player.positionMs.toFloat() / player.durationMs).coerceIn(0f, 1f) },
                modifier = Modifier.fillMaxWidth().height(2.dp),
                color = MaterialTheme.colorScheme.primary,
                trackColor = Color.Transparent,
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
                    contentDescription = if (player.playing) t("player.pause") else t("player.play"),
                )
            }
            IconButton(onClick = player::next, enabled = player.upNext.isNotEmpty()) {
                Icon(Icons.Default.SkipNext, contentDescription = t("player.next"))
            }
        }
    }
}

/** The bottom bar's top edge: a thin neon line. */
@Composable
private fun Modifier.neonTopEdge(): Modifier {
    val neon = LocalNeon.current
    return this.drawBehind {
        drawLine(neon.edge, Offset(0f, 0f), Offset(size.width, 0f), strokeWidth = 1.dp.toPx())
    }
}

/** The chosen tab glows: lavender icon and name, a soft pill behind. */
@Composable
private fun neonItemColors() = NavigationBarItemDefaults.colors(
    selectedIconColor = MaterialTheme.colorScheme.primary,
    selectedTextColor = MaterialTheme.colorScheme.primary,
    indicatorColor = MaterialTheme.colorScheme.primary.copy(alpha = 0.22f),
    unselectedIconColor = MaterialTheme.colorScheme.onSurfaceVariant,
    unselectedTextColor = MaterialTheme.colorScheme.onSurfaceVariant,
)

/** The picture behind every screen: one of the built-in ones the server serves, or the admin's own. */
@Composable
private fun HouseBackground(nav: Nav) {
    val url = nav.api.backgroundUrl(nav.appearance.background)
    val strength = nav.appearance.strength / 100f
    val neon = LocalNeon.current
    when {
        url != null -> coil3.compose.AsyncImage(
            model = url,
            contentDescription = null,
            contentScale = androidx.compose.ui.layout.ContentScale.Crop,
            alpha = strength,
            modifier = Modifier.fillMaxSize(),
        )
        nav.appearance.background == "aurora" -> Box(
            Modifier.fillMaxSize().drawBehind {
                val a = strength
                drawRect(Brush.radialGradient(listOf(neon.accent2.copy(alpha = 0.75f * a), Color.Transparent), Offset(size.width * 0.2f, size.height * 0.15f), size.width * 0.8f))
                drawRect(Brush.radialGradient(listOf(neon.glow.copy(alpha = 0.85f * a), Color.Transparent), Offset(size.width * 0.8f, size.height * 0.25f), size.width * 0.8f))
            },
        )
        else -> Unit
    }
}
