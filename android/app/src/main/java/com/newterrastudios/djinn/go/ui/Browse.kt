package com.newterrastudios.djinn.go.ui

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.Logout
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.filled.Search
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.SecondaryTabRow
import androidx.compose.material3.Tab
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.media3.common.util.UnstableApi
import com.newterrastudios.djinn.go.data.Album
import com.newterrastudios.djinn.go.data.AlbumDetail
import com.newterrastudios.djinn.go.data.ApiException
import com.newterrastudios.djinn.go.data.ArtistDetail
import com.newterrastudios.djinn.go.data.ArtistSummary
import com.newterrastudios.djinn.go.data.GoHome
import com.newterrastudios.djinn.go.data.GoPlaylistDetail
import com.newterrastudios.djinn.go.data.Page
import com.newterrastudios.djinn.go.data.SearchResults
import com.newterrastudios.djinn.go.data.Track
import kotlinx.coroutines.delay
import java.io.IOException

/** Loads one thing for a screen, with what to say while it does. */
@Composable
private fun <T : Any> Loaded(
    key: Any,
    load: suspend () -> T,
    content: @Composable (T) -> Unit,
) {
    var value by remember(key) { mutableStateOf<T?>(null) }
    var failed by remember(key) { mutableStateOf<String?>(null) }
    var attempt by remember(key) { mutableStateOf(0) }
    LaunchedEffect(key, attempt) {
        failed = null
        try {
            value = load()
        } catch (e: IOException) {
            failed = if (e is ApiException) e.message else "Couldn't reach the server."
        }
    }
    val shown = value
    when {
        shown != null -> content(shown)
        failed != null -> Column(Modifier.padding(16.dp)) {
            Text(failed ?: "", color = MaterialTheme.colorScheme.error)
            FilledTonalButton(onClick = { attempt++ }) { Text("Try again") }
        }
        else -> Note("Loading...")
    }
}

/** A long list from the server, a page at a time as you scroll. */
@Composable
private fun <T> PagedList(
    key: String,
    load: suspend (Int) -> Page<T>,
    row: @Composable (T, List<T>) -> Unit,
) {
    val items = remember(key) { mutableStateListOf<T>() }
    var total by remember(key) { mutableStateOf<Long?>(null) }
    var failed by remember(key) { mutableStateOf(false) }
    val state = rememberLazyListState()
    val wantMore by remember(key) {
        derivedStateOf {
            val last = state.layoutInfo.visibleItemsInfo.lastOrNull()?.index ?: 0
            last >= items.size - 20
        }
    }
    LaunchedEffect(key) {
        snapshotFlow { wantMore to items.size }.collect { (more, size) ->
            val known = total
            if (more && !failed && (known == null || size < known)) {
                try {
                    val page = load(size)
                    items.addAll(page.items)
                    total = if (page.items.isEmpty()) items.size.toLong() else page.total
                } catch (_: IOException) {
                    failed = true
                }
            }
        }
    }
    LazyColumn(Modifier.fillMaxSize(), state = state) {
        items(items) { row(it, items) }
        if (failed) {
            item {
                Column(Modifier.padding(16.dp)) {
                    Text("Couldn't load more.", color = MaterialTheme.colorScheme.error)
                    FilledTonalButton(onClick = { failed = false }) { Text("Try again") }
                }
            }
        } else if (total == null) {
            item { Note("Loading...") }
        } else if (total == 0L) {
            item { Note("Nothing here yet.") }
        }
    }
}

@UnstableApi
@Composable
fun HomeScreen(nav: Nav) {
    Loaded(key = "home", load = { nav.api.home() }) { home: GoHome ->
        LazyColumn(Modifier.fillMaxSize()) {
            item {
                Row(
                    Modifier
                        .fillMaxWidth()
                        .padding(start = 16.dp, top = 8.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text("DJINN GO", style = MaterialTheme.typography.headlineSmall, modifier = Modifier.weight(1f))
                    IconButton(onClick = nav.signOut) {
                        Icon(Icons.AutoMirrored.Filled.Logout, contentDescription = "Sign out")
                    }
                }
            }
            home.resume?.let { resume ->
                resume.songs.getOrNull(resume.position)?.let { song ->
                    item {
                        Heading("Carry on")
                        Row2(
                            title = song.title,
                            subtitle = song.artist,
                            art = { Art(nav.api.artUrl(song.artHash), 56.dp) },
                            onClick = { nav.player.resume(resume) },
                            end = { Icon(Icons.Default.PlayArrow, contentDescription = "Carry on") },
                        )
                    }
                }
            }
            if (home.playlists.isNotEmpty()) {
                item { Heading("Your playlists") }
                items(home.playlists) { list ->
                    Row2(
                        title = list.name,
                        subtitle = "${list.tracks} songs",
                        art = { Art(null, 44.dp) },
                        onClick = { nav.push(Screen.Playlist(list.id)) },
                    )
                }
            }
            songSection("Lately played", home.recent, nav)
            songSection("New in the library", home.newest, nav)
        }
    }
}

@UnstableApi
private fun LazyListScope.songSection(title: String, songs: List<Track>, nav: Nav) {
    if (songs.isEmpty()) return
    item { Heading(title) }
    itemsIndexed(songs) { index, song ->
        SongRow(song, nav) { nav.player.play(songs, index) }
    }
}

@UnstableApi
@Composable
fun LibraryScreen(nav: Nav) {
    var tab by rememberSaveable { mutableStateOf(0) }
    Column(Modifier.fillMaxSize()) {
        SecondaryTabRow(selectedTabIndex = tab) {
            listOf("Artists", "Albums", "Songs").forEachIndexed { index, label ->
                Tab(selected = tab == index, onClick = { tab = index }, text = { Text(label) })
            }
        }
        when (tab) {
            0 -> PagedList<ArtistSummary>("artists", nav.api::artists) { artist, _ ->
                Row2(
                    title = artist.name,
                    subtitle = "${artist.trackCount} songs",
                    art = { Art(nav.api.artUrl(artist.artHash), 44.dp, CircleShape) },
                    onClick = { nav.push(Screen.Artist(artist.id)) },
                )
            }
            1 -> PagedList<Album>("albums", nav.api::albums) { album, _ ->
                AlbumRow(album, nav)
            }
            else -> PagedList<Track>("songs", nav.api::songs) { song, _ ->
                SongRow(song, nav) { nav.startRadio(song) }
            }
        }
    }
}

@UnstableApi
@Composable
private fun AlbumRow(album: Album, nav: Nav) {
    Row2(
        title = album.title,
        subtitle = listOfNotNull(album.artist, album.year?.toString()).joinToString(" · "),
        art = { Art(nav.api.artUrl(album.artHash), 44.dp) },
        onClick = { nav.push(Screen.Album(album.id)) },
    )
}

/** A tap in a big list plays that song, then songs like it. */
@UnstableApi
private fun Nav.startRadio(song: Track) {
    player.play(listOf(song))
    radioFor = song
}

@UnstableApi
@Composable
fun AlbumScreen(id: Long, nav: Nav) {
    Loaded(key = id, load = { nav.api.album(id) }) { album: AlbumDetail ->
        LazyColumn(Modifier.fillMaxSize()) {
            item {
                Column(Modifier.padding(16.dp)) {
                    Art(nav.api.artUrl(album.artHash, 512), 200.dp)
                    Text(album.title, style = MaterialTheme.typography.headlineSmall, modifier = Modifier.padding(top = 12.dp))
                    Text(
                        listOfNotNull(album.artist, album.year?.toString()).joinToString(" · "),
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    FilledTonalButton(
                        onClick = { nav.player.play(album.tracks) },
                        enabled = album.tracks.isNotEmpty(),
                        modifier = Modifier.padding(top = 8.dp),
                    ) { Text("Play album") }
                }
            }
            itemsIndexed(album.tracks) { index, song ->
                SongRow(song, nav) { nav.player.play(album.tracks, index) }
            }
        }
    }
}

@UnstableApi
@Composable
fun ArtistScreen(id: Long, nav: Nav) {
    Loaded(key = id, load = { nav.api.artist(id) }) { artist: ArtistDetail ->
        LazyColumn(Modifier.fillMaxSize()) {
            item {
                Column(Modifier.padding(16.dp)) {
                    Art(nav.api.artUrl(artist.artHash, 512), 160.dp, CircleShape)
                    Text(artist.name, style = MaterialTheme.typography.headlineSmall, modifier = Modifier.padding(top = 12.dp))
                    artist.bio?.let {
                        Text(
                            it,
                            maxLines = 4,
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            modifier = Modifier.padding(top = 8.dp),
                        )
                    }
                }
            }
            songSection("Top songs", artist.topTracks.take(5), nav)
            if (artist.albums.isNotEmpty()) {
                item { Heading("Albums") }
                items(artist.albums) { AlbumRow(it, nav) }
            }
            if (artist.appearsOn.isNotEmpty()) {
                item { Heading("Appears on") }
                items(artist.appearsOn) { AlbumRow(it, nav) }
            }
            songSection("Other songs", artist.otherTracks, nav)
        }
    }
}

@UnstableApi
@Composable
fun PlaylistScreen(id: Long, nav: Nav) {
    Loaded(key = id, load = { nav.api.playlist(id) }) { list: GoPlaylistDetail ->
        LazyColumn(Modifier.fillMaxSize()) {
            item {
                Column(Modifier.padding(16.dp)) {
                    Text(list.name, style = MaterialTheme.typography.headlineSmall)
                    FilledTonalButton(
                        onClick = { nav.player.play(list.songs) },
                        enabled = list.songs.isNotEmpty(),
                        modifier = Modifier.padding(top = 8.dp),
                    ) { Text("Play") }
                }
            }
            itemsIndexed(list.songs) { index, song ->
                SongRow(song, nav) { nav.player.play(list.songs, index) }
            }
        }
    }
}

@UnstableApi
@Composable
fun SearchScreen(nav: Nav) {
    var query by rememberSaveable { mutableStateOf("") }
    var results by remember { mutableStateOf<SearchResults?>(null) }
    var failed by remember { mutableStateOf(false) }
    LaunchedEffect(query) {
        failed = false
        if (query.isBlank()) {
            results = null
            return@LaunchedEffect
        }
        // Wait for typing to stop.
        delay(250)
        try {
            results = nav.api.search(query.trim())
        } catch (_: IOException) {
            failed = true
        }
    }
    Column(Modifier.fillMaxSize()) {
        OutlinedTextField(
            value = query,
            onValueChange = { query = it },
            singleLine = true,
            placeholder = { Text("Songs, artists, or \"chill house\"") },
            leadingIcon = { Icon(Icons.Default.Search, contentDescription = null) },
            modifier = Modifier
                .fillMaxWidth()
                .padding(12.dp),
        )
        if (failed) Note("Couldn't reach the server.")
        val found = results ?: return@Column
        val empty = found.tracks.isEmpty() && found.albums.isEmpty() && found.artists.isEmpty() &&
            found.described?.tracks.isNullOrEmpty()
        LazyColumn(Modifier.fillMaxSize()) {
            if (empty) item { Note("Nothing found.") }
            if (found.artists.isNotEmpty()) {
                item { Heading("Artists") }
                items(found.artists) { artist ->
                    Row2(
                        title = artist.name,
                        subtitle = null,
                        art = { Art(nav.api.artUrl(artist.artHash), 44.dp, CircleShape) },
                        onClick = { nav.push(Screen.Artist(artist.id)) },
                    )
                }
            }
            if (found.albums.isNotEmpty()) {
                item { Heading("Albums") }
                items(found.albums) { AlbumRow(it, nav) }
            }
            songSection("Songs", found.tracks, nav)
            found.described?.let { songSection(it.summary.joinToString(", "), it.tracks, nav) }
        }
    }
}
