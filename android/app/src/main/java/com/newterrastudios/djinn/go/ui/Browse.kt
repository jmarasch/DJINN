package com.newterrastudios.djinn.go.ui

import com.newterrastudios.djinn.go.i18n.t
import kotlinx.coroutines.withContext
import androidx.compose.material.icons.filled.Favorite
import androidx.compose.material.icons.filled.FavoriteBorder
import androidx.compose.foundation.layout.size
import com.newterrastudios.djinn.go.data.Liked
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.clickable
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.material.icons.automirrored.filled.List
import androidx.compose.material.icons.filled.BubbleChart
import androidx.compose.runtime.mutableIntStateOf
import kotlinx.coroutines.Dispatchers
import com.newterrastudios.djinn.go.data.importText
import com.newterrastudios.djinn.go.data.ShareTarget
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.material3.Checkbox
import androidx.compose.material3.LinearProgressIndicator
import androidx.activity.result.contract.ActivityResultContracts
import androidx.activity.compose.rememberLauncherForActivityResult
import android.provider.OpenableColumns
import androidx.compose.material3.SecondaryScrollableTabRow
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.material.icons.filled.KeyboardArrowUp
import androidx.compose.material3.SmallFloatingActionButton
import com.newterrastudios.djinn.go.data.KeptEntry
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.material3.FilterChip
import androidx.compose.foundation.layout.Column
import androidx.compose.material.icons.filled.Radio
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
import androidx.compose.foundation.layout.Box
import androidx.compose.material.icons.filled.Close
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.TextButton
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.material.icons.filled.Download
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.filled.Search
import androidx.compose.material.icons.filled.Settings
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
import androidx.compose.runtime.collectAsState
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
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.media3.common.util.UnstableApi
import com.newterrastudios.djinn.go.data.Album
import com.newterrastudios.djinn.go.data.AlbumDetail
import com.newterrastudios.djinn.go.data.ApiException
import com.newterrastudios.djinn.go.data.CertificateChanged
import com.newterrastudios.djinn.go.data.ArtistDetail
import com.newterrastudios.djinn.go.data.ArtistSummary
import com.newterrastudios.djinn.go.data.GoHome
import com.newterrastudios.djinn.go.data.GoPlaylist
import com.newterrastudios.djinn.go.data.GoPlaylistDetail
import com.newterrastudios.djinn.go.data.largeDownloadWarning
import com.newterrastudios.djinn.go.data.Page
import com.newterrastudios.djinn.go.data.SearchResults
import com.newterrastudios.djinn.go.data.Track
import com.newterrastudios.djinn.go.data.GenreList
import com.newterrastudios.djinn.go.data.GenrePage
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.height
import androidx.compose.runtime.mutableStateMapOf
import kotlinx.coroutines.flow.collectLatest
import androidx.compose.material.icons.filled.Shuffle
import androidx.compose.material3.AssistChip
import androidx.work.WorkManager
import com.newterrastudios.djinn.go.work.DOWNLOAD_TAG
import com.newterrastudios.djinn.go.work.DownloadWorker
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import java.io.IOException

/** Loads one thing for a screen, with what to say while it does. */
@Composable
private fun <T : Any> Loaded(
    key: Any,
    load: suspend () -> T,
    /** What to show instead of an error, when there's something. */
    away: (@Composable (String?) -> Unit)? = null,
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
            failed = if (e is ApiException || e is CertificateChanged) e.message else "Couldn't reach the server."
        }
    }
    val shown = value
    when {
        shown != null -> content(shown)
        failed != null && away != null -> away(failed)
        failed != null -> Column(Modifier.padding(16.dp)) {
            Text(failed ?: "", color = MaterialTheme.colorScheme.error)
            FilledTonalButton(onClick = { attempt++ }) { Text("Try again") }
        }
        else -> Note("Loading...")
    }
}

/** How far down a list counts as worth a way back to the top. */
private const val TO_TOP_AFTER_ROWS = 10

/** How far either side of the screen to have rows ready. */
private const val LOAD_AROUND_ROWS = 20

/** How long the list has to stay put before loading, so sliding along the letters doesn't fetch every one. */
private const val LOAD_SETTLE_MS = 120L

/** Room for a row not loaded yet: its picture plus padding. */
private val UNLOADED_ROW = 60.dp

/**
 * A long list from the server. Each row sits at its place in the whole list, so
 * a jump can land anywhere, and pages load around what's on screen.
 */
@Composable
private fun <T : Any> PagedList(
    key: String,
    load: suspend (Int) -> Page<T>,
    /** Options above the list, out of the way until you scroll back up. */
    options: @Composable () -> Unit = {},
    /** The letters or years beside the list, for orders that have them. */
    jumps: suspend () -> List<JumpEntry> = { emptyList() },
    row: @Composable (T) -> Unit,
) {
    val items = remember(key) { mutableStateMapOf<Int, T>() }
    var total by remember(key) { mutableStateOf<Int?>(null) }
    var failed by remember(key) { mutableStateOf(false) }
    var entries by remember(key) { mutableStateOf<List<JumpEntry>>(emptyList()) }
    var pressedLabel by remember(key) { mutableStateOf<String?>(null) }
    // Each order starts from its top.
    val state = remember(key) { LazyListState() }
    val scope = rememberCoroutineScope()
    val missing by remember(key) {
        derivedStateOf {
            val known = total ?: return@derivedStateOf 0
            val visible = state.layoutInfo.visibleItemsInfo
            val first = (visible.firstOrNull()?.index ?: 0) - LOAD_AROUND_ROWS
            val last = (visible.lastOrNull()?.index ?: 0) + LOAD_AROUND_ROWS
            (first.coerceAtLeast(0)..last.coerceAtMost(known - 1)).firstOrNull { it !in items }
        }
    }
    LaunchedEffect(key) {
        snapshotFlow { if (failed) null else missing }.collectLatest { at ->
            if (at == null) return@collectLatest
            if (total != null) delay(LOAD_SETTLE_MS)
            try {
                val page = load(at)
                page.items.forEachIndexed { index, item -> items[at + index] = item }
                total = if (page.items.isEmpty()) at else page.total.toInt()
            } catch (_: IOException) {
                failed = true
            }
        }
    }
    // Asks again after Try again, when the server may be back.
    LaunchedEffect(key, failed) {
        if (failed || entries.isNotEmpty()) return@LaunchedEffect
        // The list works without them, so not reaching the server here says nothing.
        entries = try {
            jumps()
        } catch (_: IOException) {
            emptyList()
        }
    }
    val goingBack = state.canScrollBackward && state.lastScrolledBackward
    val count = total ?: 0
    val showStrip = entries.size > 1 && count > 0
    Box(Modifier.fillMaxSize()) {
        Column(Modifier.fillMaxSize()) {
            AnimatedVisibility(visible = !state.canScrollBackward || goingBack) { options() }
            if (failed) {
                Column(Modifier.padding(16.dp)) {
                    Text("Couldn't load more.", color = MaterialTheme.colorScheme.error)
                    FilledTonalButton(onClick = { failed = false }) { Text("Try again") }
                }
            }
            Row(
                Modifier
                    .fillMaxWidth()
                    .weight(1f),
            ) {
                LazyColumn(
                    Modifier
                        .weight(1f)
                        .fillMaxHeight(),
                    state = state,
                ) {
                    items(count) { index ->
                        val item = items[index]
                        if (item != null) row(item) else Spacer(Modifier.height(UNLOADED_ROW))
                    }
                    if (total == null && !failed) {
                        item { Note("Loading...") }
                    } else if (total == 0) {
                        item { Note("Nothing here yet.") }
                    }
                }
                if (showStrip) {
                    JumpStrip(
                        entries,
                        onPressed = { pressedLabel = it },
                        onJump = { offset -> scope.launch { state.scrollToItem(offset.coerceAtMost(count - 1)) } },
                    )
                }
            }
        }
        AnimatedVisibility(
            visible = goingBack && state.firstVisibleItemIndex >= TO_TOP_AFTER_ROWS,
            modifier = Modifier
                .align(Alignment.BottomEnd)
                .padding(end = if (showStrip) 44.dp else 16.dp, bottom = 16.dp),
        ) {
            SmallFloatingActionButton(onClick = { scope.launch { state.scrollToItem(0) } }) {
                Icon(Icons.Default.KeyboardArrowUp, contentDescription = "Back to top")
            }
        }
        pressedLabel?.let { JumpBubble(it, Modifier.align(Alignment.Center)) }
    }
}

@UnstableApi
@Composable
fun HomeScreen(nav: Nav) {
    Loaded(
        key = "home",
        load = { nav.api.home() },
        away = { reason ->
            Column(Modifier.fillMaxSize()) {
                HomeBar(nav)
                Note("$reason Here's what's on this phone.")
                KeptSongs(nav)
            }
        },
    ) { home: GoHome ->
        LazyColumn(Modifier.fillMaxSize()) {
            item { HomeBar(nav) }
            item { UpdateOffer(nav) }
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
@Composable
private fun HomeBar(nav: Nav) {
    Row(
        Modifier
            .fillMaxWidth()
            .padding(start = 16.dp, top = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text("DJINN GO", style = MaterialTheme.typography.headlineSmall, modifier = Modifier.weight(1f))
    }
}

/** Everything on the phone, which needs no server. */
@UnstableApi
@Composable
private fun KeptSongs(nav: Nav) {
    val flow = remember(nav.server) { nav.offline.dao.songsFlow(nav.server) }
    val kept by flow.collectAsState(emptyList())
    val locked = nav.offline.isLocked()
    val songs = if (locked) emptyList() else kept.map { it.toTrack() }
    LazyColumn(Modifier.fillMaxSize()) {
        if (locked && kept.isNotEmpty()) {
            item {
                val songs = if (kept.size == 1) "1 kept song is" else "${kept.size} kept songs are"
                Note("Your $songs locked because this phone hasn't reached the server for a while. They'll unlock when it does.")
            }
        } else if (songs.isEmpty()) {
            item { Note("Nothing here yet. A song's menu can download it, and albums have a download button.") }
        }
        itemsIndexed(songs) { index, song ->
            SongRow(song, nav) { nav.player.play(songs, index) }
        }
    }
}

/** Everything kept on the phone, from every server, that its rules let play. */
@UnstableApi
@Composable
private fun AllKeptSongs(nav: Nav) {
    var entries by remember { mutableStateOf<List<KeptEntry>?>(null) }
    var locked by remember { mutableStateOf(0) }
    LaunchedEffect(nav.keptIds) {
        val (open, shut) = nav.offline.allKept()
        entries = open
        locked = shut
    }
    val shown = entries ?: return
    val songs = shown.map { it.song }
    LazyColumn(Modifier.fillMaxSize()) {
        if (shown.isEmpty() && locked == 0) {
            item { Note("Nothing here yet. A song's menu can download it, and albums have a download button.") }
        }
        if (locked > 0) {
            val count = if (locked == 1) "1 song is" else "$locked songs are"
            item { Note("$count locked until you sign in to its server, or that server is reached again.") }
        }
        itemsIndexed(shown) { index, (song, serverName) ->
            val here = song.server == nav.server
            Row2(
                title = song.title,
                subtitle = listOfNotNull(song.artist, serverName).joinToString(" · "),
                art = { Art(if (here) nav.api.artUrl(song.artHash) else null, 44.dp) },
                onClick = { nav.player.playKept(songs, index, nav.server) },
            )
        }
    }
}

/** Downloads an album's or playlist's songs, when the role allows. */
@UnstableApi
@Composable
private fun DownloadAll(songs: List<Track>, nav: Nav) {
    if (!nav.canKeepSongs || songs.isEmpty()) return
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var warning by remember { mutableStateOf<String?>(null) }
    fun start() {
        DownloadWorker.enqueue(context, songs, nav.prefs.wifiOnly)
        nav.message = "Downloading ${songs.size} songs" +
            if (nav.prefs.wifiOnly) " when on Wi-Fi" else ""
    }
    FilledTonalButton(
        onClick = {
            scope.launch {
                warning = try {
                    val bytes = nav.api.versions(songs.map { it.id }).sumOf { it.size }
                    val canConvert = nav.api.playLinks(songs.first().id).canConvert
                    largeDownloadWarning(bytes, nav.prefs.kbps, canConvert)
                } catch (_: IOException) {
                    null
                }
                if (warning == null) start()
            }
        },
        modifier = Modifier.padding(top = 8.dp),
    ) {
        Icon(Icons.Default.Download, contentDescription = null)
        Text("Download", modifier = Modifier.padding(start = 8.dp))
    }
    warning?.let { text ->
        AlertDialog(
            onDismissRequest = { warning = null },
            title = { Text("Download ${songs.size} songs?") },
            text = { Text(text) },
            confirmButton = {
                TextButton(onClick = {
                    warning = null
                    start()
                }) { Text("Download") }
            },
            dismissButton = { TextButton(onClick = { warning = null }) { Text("Not now") } },
        )
    }
}

/** How many songs are on their way, with a way to stop. */
@Composable
fun DownloadStatus() {
    val context = LocalContext.current
    val manager = remember { WorkManager.getInstance(context) }
    val work by remember { manager.getWorkInfosByTagFlow(DOWNLOAD_TAG) }.collectAsState(emptyList())
    val waiting = work.count { !it.state.isFinished }
    if (waiting == 0) return
    Row(
        Modifier
            .fillMaxWidth()
            .padding(horizontal = 16.dp, vertical = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(if (waiting == 1) "1 download waiting or running" else "$waiting downloads waiting or running", modifier = Modifier.weight(1f))
        TextButton(onClick = { manager.cancelAllWorkByTag(DOWNLOAD_TAG) }) { Text("Stop") }
    }
}

/** Your playlists, with a way to start a new one. */
@UnstableApi
@Composable
private fun PlaylistsTab(nav: Nav) {
    var naming by remember { mutableStateOf(false) }
    var version by remember { mutableStateOf(0) }
    val scope = rememberCoroutineScope()
    val context = LocalContext.current
    var pasting by remember { mutableStateOf(false) }
    fun import(text: String, filename: String?) {
        // Started from the app's scope: leaving this tab shouldn't lose it.
        nav.scope.launch {
            nav.message = "Importing..."
            try {
                val result = nav.api.importPlaylist(text, filename)
                nav.message = result.playlists.joinToString(". ") {
                    "${it.name}: ${it.found} found" + if (it.missing > 0) ", ${it.missing} not in the library" else ""
                }
                version++
            } catch (e: IOException) {
                nav.message = if (e is ApiException) e.message else "Couldn't reach the server."
            }
        }
    }
    val pick = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        if (uri != null) {
            nav.scope.launch {
                val file = withContext(Dispatchers.IO) {
                    runCatching {
                        val resolver = context.contentResolver
                        val name = resolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use {
                            if (it.moveToFirst()) it.getString(0) else null
                        }
                        val bytes = resolver.openInputStream(uri)?.use { it.readBytes() }
                        bytes?.let { importText(it) to name }
                    }.getOrNull()
                }
                if (file != null) import(file.first, file.second) else nav.message = "Couldn't read that file."
            }
        }
    }
    Loaded(key = "playlists-$version", load = { nav.api.playlists() }) { lists: List<GoPlaylist> ->
        LazyColumn(Modifier.fillMaxSize()) {
            item {
                Row(
                    Modifier.padding(16.dp),
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    FilledTonalButton(onClick = { naming = true }) { Text("New playlist") }
                    FilledTonalButton(onClick = { pasting = true }) { Text("Import") }
                }
            }
            if (lists.isEmpty()) item { Note("No playlists yet.") }
            items(lists) { list ->
                Row2(
                    title = list.name,
                    subtitle = listOfNotNull(
                        if (list.tracks == 1L) "1 song" else "${list.tracks} songs",
                        if (list.mine) null else "from ${list.owner}",
                    ).joinToString(" · "),
                    art = { Art(null, 44.dp) },
                    onClick = { nav.push(Screen.Playlist(list.id)) },
                )
            }
        }
    }
    if (pasting) {
        ImportDialog(
            nav,
            onFile = {
                pasting = false
                pick.launch(arrayOf("*/*"))
            },
            onPaste = { text ->
                pasting = false
                import(text, null)
            },
            onCancel = { pasting = false },
        )
    }
    if (naming) {
        NameDialog("New playlist", "", onDone = { name ->
            naming = false
            if (name != null) {
                scope.launch {
                    try {
                        nav.api.createPlaylist(name)
                        version++
                    } catch (e: IOException) {
                        nav.message = if (e is ApiException) e.message else "Couldn't reach the server."
                    }
                }
            }
        })
    }
}

/** Bring in a playlist: names only, never music. */
@Composable
private fun ImportDialog(nav: Nav, onFile: () -> Unit, onPaste: (String) -> Unit, onCancel: () -> Unit) {
    var text by remember { mutableStateOf("") }
    var youtubeOn by remember { mutableStateOf<Boolean?>(null) }
    LaunchedEffect(Unit) {
        youtubeOn = try {
            nav.api.youtubeLinks().hasKey
        } catch (_: IOException) {
            null
        }
    }
    AlertDialog(
        onDismissRequest = onCancel,
        title = { Text("Import a playlist") },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(
                    "This brings in the list, not the music. Songs you don't have stay in the list, flagged, " +
                        "and are skipped. For other services, get a file from Tune My Music or Soundiiz.",
                    style = MaterialTheme.typography.bodySmall,
                )
                when (youtubeOn) {
                    true -> Text("You can paste a YouTube playlist link too.", style = MaterialTheme.typography.bodySmall)
                    false -> Text(
                        "YouTube links are off on this server. An admin can turn them on under Admin > Library.",
                        style = MaterialTheme.typography.bodySmall,
                    )
                    null -> Unit
                }
                FilledTonalButton(onClick = onFile) { Text("Choose a file") }
                OutlinedTextField(
                    value = text,
                    onValueChange = { text = it },
                    minLines = 3,
                    maxLines = 6,
                    label = { Text(if (youtubeOn == false) "Or paste songs" else "Or paste songs, or a YouTube link") },
                    placeholder = { Text("Artist - Title") },
                )
            }
        },
        confirmButton = {
            TextButton(enabled = text.isNotBlank(), onClick = { onPaste(text) }) { Text("Import") }
        },
        dismissButton = { TextButton(onClick = onCancel) { Text("Cancel") } },
    )
}

/** Who can see a playlist of yours: only you, everyone, or particular people. */
@UnstableApi
@Composable
private fun SharingDialog(list: GoPlaylistDetail, nav: Nav, onClose: () -> Unit) {
    val scope = rememberCoroutineScope()
    var everyone by remember { mutableStateOf(list.shared) }
    var people by remember { mutableStateOf<List<ShareTarget>?>(null) }
    val chosen = remember { mutableStateListOf<Long>() }
    fun change(action: suspend () -> Unit, after: () -> Unit) {
        scope.launch {
            try {
                action()
                after()
            } catch (e: IOException) {
                nav.message = if (e is ApiException) e.message else "Couldn't reach the server."
            }
        }
    }
    LaunchedEffect(list.id) {
        try {
            val shares = nav.api.playlistShares(list.id)
            chosen.addAll(shares.map { it.id })
            people = runCatching { nav.api.shareTargets() }.getOrDefault(shares)
        } catch (e: IOException) {
            nav.message = if (e is ApiException) e.message else "Couldn't reach the server."
            onClose()
        }
    }
    AlertDialog(
        onDismissRequest = onClose,
        title = { Text("Share ${list.name}") },
        text = {
            LazyColumn {
                item {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Checkbox(
                            checked = everyone,
                            onCheckedChange = { on ->
                                change({ nav.api.setPlaylistShared(list.id, on) }) { everyone = on }
                            },
                        )
                        Text("Everyone on this server")
                    }
                }
                items(people.orEmpty()) { person ->
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Checkbox(
                            checked = person.id in chosen,
                            enabled = !everyone,
                            onCheckedChange = { on ->
                                if (on) {
                                    change({ nav.api.sharePlaylist(list.id, person.id) }) { chosen.add(person.id) }
                                } else {
                                    change({ nav.api.unsharePlaylist(list.id, person.id) }) { chosen.remove(person.id) }
                                }
                            },
                        )
                        Text(person.name)
                    }
                }
                item {
                    Text(
                        "They can see and play it. Only you can change it.",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
        },
        confirmButton = { TextButton(onClick = onClose) { Text("Done") } },
    )
}

@Composable
fun NameDialog(title: String, start: String, onDone: (String?) -> Unit) {
    var name by remember { mutableStateOf(start) }
    AlertDialog(
        onDismissRequest = { onDone(null) },
        title = { Text(title) },
        text = {
            OutlinedTextField(value = name, onValueChange = { name = it }, singleLine = true, label = { Text("Name") })
        },
        confirmButton = {
            TextButton(enabled = name.isNotBlank(), onClick = { onDone(name.trim()) }) { Text("Save") }
        },
        dismissButton = { TextButton(onClick = { onDone(null) }) { Text("Cancel") } },
    )
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
        val tabs: @Composable () -> Unit = {
            LIBRARY_TABS.forEachIndexed { index, label ->
                Tab(
                    selected = tab == index,
                    onClick = { tab = index },
                    text = { Text(label, maxLines = 1, softWrap = false) },
                )
            }
        }
        // Six words only share a phone evenly at small text sizes. Rather than break them, they scroll.
        val configuration = LocalConfiguration.current
        if (configuration.screenWidthDp / configuration.fontScale >= ROOM_FOR_EVEN_TABS_DP) {
            SecondaryTabRow(selectedTabIndex = tab, tabs = tabs)
        } else {
            // No minimum width: each tab just fits its word, so the next one shows at the edge when they scroll.
            SecondaryScrollableTabRow(selectedTabIndex = tab, edgePadding = 0.dp, minTabWidth = 0.dp, tabs = tabs)
        }
        when (tab) {
            0 -> {
                val sort = nav.artistSort
                PagedList<ArtistSummary>(
                    "artists-$sort",
                    { nav.api.artists(sort, it) },
                    options = { Choices(ARTIST_SORTS, nav.artistSort) { nav.artistSort = it } },
                    jumps = { jumpEntries(nav.api.artistJumps(sort), sort) },
                ) { artist ->
                    Row2(
                        title = artist.name,
                        subtitle = (listOf("${artist.trackCount} songs") + countParts(artist.plays, artist.likes))
                            .joinToString(" · "),
                        art = { Art(nav.api.artUrl(artist.artHash), 44.dp, CircleShape) },
                        onClick = { nav.push(Screen.Artist(artist.id)) },
                        end = { LikeHeart(artist.id, nav.likedArtists, artist.name, nav.api::setArtistLike, nav) },
                    )
                }
            }
            1 -> {
                val sort = nav.albumSort
                val filter = nav.albumFilter
                PagedList<Album>(
                    "albums-$sort-$filter",
                    { nav.api.albums(sort, filter, it) },
                    options = {
                        Column {
                            Choices(ALBUM_FILTERS, nav.albumFilter) { nav.albumFilter = it }
                            Choices(ALBUM_SORTS, nav.albumSort) { nav.albumSort = it }
                        }
                    },
                    jumps = { jumpEntries(nav.api.albumJumps(sort, filter), sort) },
                ) { album ->
                    AlbumRow(album, nav)
                }
            }
            2 -> {
                val sort = nav.songSort
                PagedList<Track>(
                    "songs-$sort",
                    { nav.api.songs(sort, it) },
                    options = { Choices(SONG_SORTS, nav.songSort) { nav.songSort = it } },
                    jumps = { jumpEntries(nav.api.songJumps(sort), sort) },
                ) { song ->
                    SongRow(song, nav) { nav.startRadio(song) }
                }
            }
            else -> GenresTab(nav)
        }
    }
}

/** Your music, as in DJINN GO's browser: what you like, your playlists,
 * what you played, your year, and what's on this phone. */
@UnstableApi
@Composable
fun YourMusicScreen(nav: Nav) {
    var tab by rememberSaveable { mutableStateOf(0) }
    Column(Modifier.fillMaxSize()) {
        SecondaryScrollableTabRow(selectedTabIndex = tab, edgePadding = 0.dp, minTabWidth = 0.dp) {
            MINE_TABS.forEachIndexed { index, label ->
                Tab(
                    selected = tab == index,
                    onClick = { tab = index },
                    text = { Text(label, maxLines = 1, softWrap = false) },
                )
            }
        }
        when (tab) {
            0 -> LikedTab(nav)
            1 -> PlaylistsTab(nav)
            2 -> HistoryTab(nav)
            3 -> YearScreen(nav)
            else -> Column {
                DownloadStatus()
                Choices(KEPT_VIEWS, nav.keptView) { nav.keptView = it }
                if (nav.keptView == "all") AllKeptSongs(nav) else KeptSongs(nav)
            }
        }
    }
}

private val MINE_TABS = listOf("Liked", "Playlists", "History", "Your year", "On phone")

/** The artists, albums and songs you like. Taking a heart off leaves the list at once. */
@UnstableApi
@Composable
private fun LikedTab(nav: Nav) {
    Loaded(key = "liked", load = { nav.api.liked() }) { liked: Liked ->
        val artists = liked.artists.filter { it.id in nav.likedArtists }
        val albums = liked.albums.filter { it.id in nav.likedAlbums }
        val songs = liked.tracks.filter { it.id in nav.liked }
        LazyColumn(Modifier.fillMaxSize()) {
            if (artists.isEmpty() && albums.isEmpty() && songs.isEmpty()) {
                item { Note("Tap the heart on a song, album or artist to keep it here.") }
            }
            if (artists.isNotEmpty()) {
                item { Heading("Artists") }
                items(artists, key = { "artist-${it.id}" }) { artist ->
                    Row2(
                        title = artist.name,
                        subtitle = "${artist.trackCount} songs",
                        art = { Art(nav.api.artUrl(artist.artHash), 44.dp, CircleShape) },
                        onClick = { nav.push(Screen.Artist(artist.id)) },
                        end = { LikeHeart(artist.id, nav.likedArtists, artist.name, nav.api::setArtistLike, nav) },
                    )
                }
            }
            if (albums.isNotEmpty()) {
                item { Heading("Albums") }
                items(albums, key = { "album-${it.id}" }) { AlbumRow(it, nav) }
            }
            if (songs.isNotEmpty()) {
                item {
                    Row(
                        Modifier.fillMaxWidth().padding(end = 8.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Box(Modifier.weight(1f)) { Heading("Songs") }
                        TextButton(onClick = { nav.player.play(songs, 0) }) { Text("Play all") }
                    }
                }
                itemsIndexed(songs, key = { _, song -> "song-${song.id}" }) { index, song ->
                    SongRow(song, nav) { nav.player.play(songs, index) }
                }
            }
        }
    }
}

/** Songs you played on GO, newest first. */
@UnstableApi
@Composable
private fun HistoryTab(nav: Nav) {
    var more by remember { mutableStateOf(true) }
    val songs = remember { mutableStateListOf<Track>() }
    var failed by remember { mutableStateOf(false) }
    suspend fun load() {
        try {
            val page = nav.api.history(songs.size)
            songs.addAll(page)
            more = page.size == 100
        } catch (_: IOException) {
            failed = true
        }
    }
    LaunchedEffect(Unit) { if (songs.isEmpty()) load() }
    val scope = rememberCoroutineScope()
    LazyColumn(Modifier.fillMaxSize()) {
        if (failed && songs.isEmpty()) item { Note("Couldn't reach the server.") }
        if (!failed && songs.isEmpty() && !more) item { Note("Songs you play show up here.") }
        itemsIndexed(songs) { index, song ->
            SongRow(song, nav) { nav.player.play(songs.toList(), index) }
        }
        if (more && songs.isNotEmpty()) {
            item {
                TextButton(onClick = { scope.launch { load() } }, modifier = Modifier.fillMaxWidth()) {
                    Text("Show more")
                }
            }
        }
    }
}

private val LIBRARY_TABS = listOf("Artists", "Albums", "Songs", "Genres")
private const val ROOM_FOR_EVEN_TABS_DP = 576

val KEPT_VIEWS = listOf("here" to "This server", "all" to "All servers")
val ARTIST_SORTS = listOf("name" to "Name", "added" to "Recently added")
val ALBUM_SORTS = listOf(
    "name" to "Name",
    "added" to "Recently added",
    "year" to "Newest",
    "oldest" to "Oldest",
)
val ALBUM_FILTERS = listOf("all" to "All albums", "various" to "Various artists", "soundtracks" to "Soundtracks")
val SONG_SORTS = listOf(
    "title" to "Title",
    "artist" to "Artist",
    "added" to "Recently added",
    "popular" to "Most popular",
    "year" to "Newest",
    "oldest" to "Oldest",
)

/** A heart for an album or artist. Shows at once, and goes back if the server can't be told. */
@UnstableApi
@Composable
internal fun LikeHeart(id: Long, likes: MutableList<Long>, name: String, set: suspend (Long, Boolean) -> Unit, nav: Nav) {
    val liked = id in likes
    IconButton(onClick = {
        val on = !liked
        if (on) likes.add(id) else likes.remove(id)
        nav.scope.launch {
            try {
                set(id, on)
            } catch (_: IOException) {
                if (on) likes.remove(id) else likes.add(id)
                nav.message = "Couldn't reach the server, so that wasn't changed."
            }
        }
    }) {
        Icon(
            if (liked) Icons.Default.Favorite else Icons.Default.FavoriteBorder,
            if (liked) "Unlike $name" else "Like $name",
            tint = if (liked) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

/** A row of choices above a list, which scrolls if they don't all fit. */
@Composable
private fun Choices(choices: List<Pair<String, String>>, chosen: String, choose: (String) -> Unit) {
    Row(
        Modifier
            .fillMaxWidth()
            .horizontalScroll(rememberScrollState())
            .padding(horizontal = 12.dp),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        choices.forEach { (value, label) ->
            FilterChip(selected = value == chosen, onClick = { choose(value) }, label = { Text(label) })
        }
    }
}

@UnstableApi
@Composable
private fun AlbumRow(album: Album, nav: Nav) {
    Row2(
        title = album.title,
        subtitle = (listOfNotNull(album.artist, album.year?.toString(), album.label) + countParts(album.plays, album.likes))
            .joinToString(" · "),
        art = { Art(nav.api.artUrl(album.artHash), 44.dp) },
        onClick = { nav.push(Screen.Album(album.id)) },
        end = { LikeHeart(album.id, nav.likedAlbums, album.title, nav.api::setAlbumLike, nav) },
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
                        listOfNotNull(album.artist, album.year?.toString(), album.label).joinToString(" · "),
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    Row(Modifier.padding(top = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                        FilledTonalButton(
                            onClick = { nav.player.play(album.tracks) },
                            enabled = album.tracks.isNotEmpty(),
                        ) { Text("Play album") }
                        album.tracks.firstOrNull()?.let { first ->
                            TextButton(onClick = { nav.startRadio(first) }) {
                                Icon(Icons.Default.Radio, contentDescription = null, Modifier.size(18.dp))
                                Text("Radio", Modifier.padding(start = 6.dp))
                            }
                        }
                        LikeHeart(album.id, nav.likedAlbums, album.title, nav.api::setAlbumLike, nav)
                    }
                    DownloadAll(album.tracks, nav)
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
    // Every song by the artist: the top ones first, the rest on Show more.
    var total by remember(id) { mutableStateOf(0L) }
    var all by remember(id) { mutableStateOf<List<Track>?>(null) }
    LaunchedEffect(id) {
        total = try {
            nav.api.artistSongs(id, 0, 1).total
        } catch (_: IOException) {
            0L
        }
    }
    val scope = nav.scope
    Loaded(key = id, load = { nav.api.artist(id) }) { artist: ArtistDetail ->
        val shown = all ?: artist.topTracks
        LazyColumn(Modifier.fillMaxSize()) {
            item {
                Column(Modifier.padding(16.dp)) {
                    Art(nav.api.artUrl(artist.artHash, 512), 160.dp, CircleShape)
                    Text(artist.name, style = MaterialTheme.typography.headlineSmall, modifier = Modifier.padding(top = 12.dp))
                    Row(Modifier.padding(top = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                        FilledTonalButton(
                            enabled = total > 0,
                            onClick = {
                                scope.launch {
                                    try {
                                        val songs = nav.api.artistSongs(id, 0, 5000).tracks
                                        if (songs.isNotEmpty()) nav.player.play(songs, 0)
                                    } catch (_: IOException) {
                                        nav.message = "Couldn't reach the server."
                                    }
                                }
                            },
                        ) { Text("Play all") }
                        artist.topTracks.firstOrNull()?.let { first ->
                            TextButton(onClick = { nav.startRadio(first) }) {
                                Icon(Icons.Default.Radio, contentDescription = null, Modifier.size(18.dp))
                                Text("Radio", Modifier.padding(start = 6.dp))
                            }
                        }
                        LikeHeart(artist.id, nav.likedArtists, artist.name, nav.api::setArtistLike, nav)
                    }
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
            if (shown.isNotEmpty()) {
                item { Heading(if (total > 0) "Songs ($total)" else "Songs") }
                itemsIndexed(shown, key = { _, song -> song.id }) { index, song ->
                    SongRow(song, nav) { nav.player.play(shown, index) }
                }
                if (shown.size < total) {
                    item {
                        TextButton(
                            onClick = {
                                scope.launch {
                                    try {
                                        val page = nav.api.artistSongs(id, all?.size ?: 0, 100)
                                        all = (all ?: emptyList()) + page.tracks
                                        total = page.total
                                    } catch (_: IOException) {
                                        nav.message = "Couldn't reach the server."
                                    }
                                }
                            },
                            modifier = Modifier.fillMaxWidth(),
                        ) { Text(if (all == null) "Show all $total songs" else "Show more") }
                    }
                }
            }
            if (artist.albums.isNotEmpty()) {
                item { Heading("Albums") }
                items(artist.albums) { AlbumRow(it, nav) }
            }
            if (artist.appearsOn.isNotEmpty()) {
                item { Heading("Appears on") }
                items(artist.appearsOn) { AlbumRow(it, nav) }
            }
        }
    }
}

/** Genre names are stored in lower case. */
private fun genreTitle(name: String) = name.replaceFirstChar { it.uppercase() }

/** Every genre in the library. */
@UnstableApi
@Composable
private fun GenresTab(nav: Nav) {
    var cloud by remember { mutableStateOf(nav.prefs.genreCloud) }
    var top by remember { mutableIntStateOf(nav.prefs.genreTop) }
    var lines by remember { mutableIntStateOf(nav.prefs.genreLines) }
    Loaded(key = "genres-$top-$lines", load = { nav.api.genres(top, lines) }) { list: GenreList ->
        val entries = remember(list) { jumpEntries(letterJumps(list.genres.map { it.name }), "name") }
        val state = rememberLazyListState()
        val scope = rememberCoroutineScope()
        var pressedLabel by remember { mutableStateOf<String?>(null) }
        Column(Modifier.fillMaxSize()) {
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End) {
            IconButton(onClick = {
                cloud = !cloud
                nav.prefs.genreCloud = cloud
            }) {
                if (cloud) {
                    Icon(Icons.AutoMirrored.Filled.List, "Show as a list")
                } else {
                    Icon(Icons.Default.BubbleChart, "Show as a cloud")
                }
            }
        }
        if (cloud && list.genres.isNotEmpty()) {
            GenreCloud(
                list,
                nav,
                top,
                lines,
                onTop = {
                    top = it
                    nav.prefs.genreTop = it
                },
                onLines = {
                    lines = it
                    nav.prefs.genreLines = it
                },
                modifier = Modifier.weight(1f),
            )
            return@Column
        }
        Box(
            Modifier
                .weight(1f)
                .fillMaxWidth(),
        ) {
            Row(Modifier.fillMaxSize()) {
                LazyColumn(
                    Modifier
                        .weight(1f)
                        .fillMaxHeight(),
                    state = state,
                ) {
                    if (list.genres.isEmpty()) item { Note("No genres yet.") }
                    items(list.genres) { genre ->
                        Row2(
                            title = genreTitle(genre.name),
                            subtitle = if (genre.songs == 1L) "1 song" else "${genre.songs} songs",
                            art = {},
                            onClick = { nav.push(Screen.Genre(genre.name)) },
                        )
                    }
                }
                if (entries.size > 1) {
                    JumpStrip(
                        entries,
                        onPressed = { pressedLabel = it },
                        onJump = { offset -> scope.launch { state.scrollToItem(offset) } },
                    )
                }
            }
            pressedLabel?.let { JumpBubble(it, Modifier.align(Alignment.Center)) }
        }
        }
    }
}

/** Songs from the end when a genre's list asks for more. */
private const val GENRE_LOAD_AHEAD = 20

/** A genre's artists, side to side, loading more toward the end. */
@UnstableApi
@Composable
private fun GenreArtists(name: String, page: GenrePage, nav: Nav) {
    var artists by remember(name) { mutableStateOf(page.artists) }
    var fetching by remember(name) { mutableStateOf(false) }
    val row = rememberLazyListState()
    LaunchedEffect(name) {
        snapshotFlow { row.layoutInfo.visibleItemsInfo.lastOrNull()?.index ?: 0 }.collect { last ->
            if (fetching || last < artists.size - 8 || artists.size >= page.artistTotal) return@collect
            fetching = true
            try {
                artists = artists + nav.api.genreArtists(name, artists.size).items
            } catch (_: IOException) {
                // Scrolling again asks again.
            } finally {
                fetching = false
            }
        }
    }
    LazyRow(
        state = row,
        contentPadding = PaddingValues(horizontal = 16.dp),
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        items(artists, key = { it.id }) { artist ->
            Column(
                Modifier
                    .width(88.dp)
                    .clickable { nav.push(Screen.Artist(artist.id)) },
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                Art(nav.api.artUrl(artist.artHash), 80.dp, CircleShape)
                Text(
                    artist.name,
                    style = MaterialTheme.typography.bodySmall,
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                    textAlign = TextAlign.Center,
                )
            }
        }
    }
}

/** One genre: its songs, artists and related genres. */
@UnstableApi
@Composable
fun GenreScreen(name: String, nav: Nav) {
    Loaded(key = name, load = { nav.api.genre(name) }) { page: GenrePage ->
        // The header scrolls with the songs: it's too tall to come and go as a
        // list's options do, which moves everything under your finger.
        val songs = remember(name) { mutableStateListOf<Track>().apply { addAll(page.tracks) } }
        var failed by remember(name) { mutableStateOf(false) }
        val list = rememberLazyListState()
        LaunchedEffect(name, failed) {
            if (failed) return@LaunchedEffect
            snapshotFlow { list.layoutInfo.visibleItemsInfo.lastOrNull()?.index ?: 0 }.collect { last ->
                // Songs start after the header's rows, so this is a little early, which is the idea.
                if (last < songs.size - GENRE_LOAD_AHEAD || songs.size >= page.total) return@collect
                try {
                    val next = nav.api.genreSongs(name, songs.size).items
                    val known = songs.mapTo(HashSet()) { it.id }
                    songs.addAll(next.filter { it.id !in known })
                    if (next.isEmpty()) failed = true
                } catch (_: IOException) {
                    failed = true
                }
            }
        }
        LazyColumn(Modifier.fillMaxSize(), state = list) {
            item {
                Column(Modifier.padding(16.dp)) {
                    Text(genreTitle(name), style = MaterialTheme.typography.headlineSmall)
                    Text(
                        (if (page.total == 1L) "1 song" else "${page.total} songs") +
                            ", shuffled with the most played nearer the top.",
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    Row(
                        Modifier.padding(top = 8.dp),
                        horizontalArrangement = Arrangement.spacedBy(8.dp),
                    ) {
                        FilledTonalButton(
                            onClick = { nav.player.play(songs.toList()) },
                            enabled = songs.isNotEmpty(),
                        ) {
                            Icon(Icons.Default.PlayArrow, contentDescription = null)
                            Text("Play", modifier = Modifier.padding(start = 8.dp))
                        }
                        FilledTonalButton(
                            onClick = { nav.player.playShuffled(songs.toList()) },
                            enabled = songs.isNotEmpty(),
                        ) {
                            Icon(Icons.Default.Shuffle, contentDescription = null)
                            Text("Shuffle", modifier = Modifier.padding(start = 8.dp))
                        }
                    }
                }
            }
            if (page.related.isNotEmpty()) {
                item { Heading("Related genres") }
                item {
                    Chips(page.related.map { genreTitle(it.name) to { nav.push(Screen.Genre(it.name)) } })
                }
            }
            if (page.artists.isNotEmpty()) {
                item { Heading("Artists (${page.artistTotal})") }
                item { GenreArtists(name, page, nav) }
            }
            if (songs.isNotEmpty()) item { Heading("Songs") }
            itemsIndexed(songs, key = { _, song -> song.id }) { index, song ->
                SongRow(song, nav) { nav.player.play(songs.toList(), index) }
            }
            if (failed && songs.size < page.total) {
                item {
                    Column(Modifier.padding(16.dp)) {
                        Text("Couldn't load more.", color = MaterialTheme.colorScheme.error)
                        FilledTonalButton(onClick = { failed = false }) { Text("Try again") }
                    }
                }
            }
        }
    }
}

/** Names that open something, wrapping onto as many lines as they need. */
@Composable
private fun Chips(chips: List<Pair<String, () -> Unit>>) {
    FlowRow(
        Modifier.padding(horizontal = 16.dp),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        chips.forEach { (label, open) ->
            AssistChip(onClick = open, label = { Text(label) })
        }
    }
}

@UnstableApi
@Composable
fun PlaylistScreen(id: Long, nav: Nav) {
    var version by remember { mutableStateOf(0) }
    var renaming by remember { mutableStateOf(false) }
    var deleting by remember { mutableStateOf(false) }
    var sharing by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    fun change(action: suspend () -> Unit) {
        scope.launch {
            try {
                action()
                version++
            } catch (e: IOException) {
                nav.message = if (e is ApiException) e.message else "Couldn't reach the server."
            }
        }
    }
    Loaded(key = "$id-$version", load = { nav.api.playlist(id) }) { list: GoPlaylistDetail ->
        LazyColumn(Modifier.fillMaxSize()) {
            item {
                Column(Modifier.padding(16.dp)) {
                    Text(list.name, style = MaterialTheme.typography.headlineSmall)
                    if (!list.mine) {
                        Text(
                            "From ${list.owner}",
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                    val flagged = list.rows.count { it.problem != null }
                    if (flagged > 0) {
                        Text(
                            "$flagged can't play and will be skipped",
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.error,
                        )
                    }
                    FilledTonalButton(
                        onClick = { nav.player.play(list.songs) },
                        enabled = list.songs.isNotEmpty(),
                        modifier = Modifier.padding(top = 8.dp),
                    ) { Text("Play") }
                    DownloadAll(list.songs, nav)
                    if (list.mine) {
                        Row {
                            TextButton(onClick = { renaming = true }) { Text("Rename") }
                            if (nav.canShare || list.shared) {
                                TextButton(onClick = { sharing = true }) { Text("Share") }
                            }
                            TextButton(onClick = { deleting = true }) { Text("Delete") }
                        }
                    }
                }
            }
            items(list.rows) { row ->
                val index = row.songIndex
                val song = index?.let { list.songs.getOrNull(it) }
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Box(Modifier.weight(1f)) {
                        if (song != null && index != null) {
                            SongRow(song, nav) { nav.player.play(list.songs, index) }
                        } else {
                            // Nothing to play, so the only thing to do here is take it out.
                            Column(Modifier.padding(horizontal = 16.dp, vertical = 8.dp)) {
                                Text(
                                    row.title,
                                    maxLines = 1,
                                    overflow = TextOverflow.Ellipsis,
                                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                                )
                                row.artist?.let {
                                    Text(
                                        it,
                                        maxLines = 1,
                                        overflow = TextOverflow.Ellipsis,
                                        style = MaterialTheme.typography.bodySmall,
                                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                                    )
                                }
                                Text(
                                    row.problemText.orEmpty(),
                                    style = MaterialTheme.typography.bodySmall,
                                    color = MaterialTheme.colorScheme.error,
                                )
                            }
                        }
                    }
                    if (list.mine && row.removable) {
                        IconButton(onClick = { change { nav.api.removePlaylistEntry(id, row.entryId) } }) {
                            Icon(Icons.Default.Close, "Take ${row.title} out of the playlist")
                        }
                    }
                }
            }
        }
        if (sharing) SharingDialog(list, nav) { sharing = false; version++ }
        if (renaming) {
            NameDialog("Rename playlist", list.name, onDone = { name ->
                renaming = false
                if (name != null) change { nav.api.renamePlaylist(id, name) }
            })
        }
        if (deleting) {
            AlertDialog(
                onDismissRequest = { deleting = false },
                title = { Text("Delete ${list.name}?") },
                text = { Text("Songs stay in the library and on this phone.") },
                confirmButton = {
                    TextButton(onClick = {
                        deleting = false
                        scope.launch {
                            try {
                                nav.api.deletePlaylist(id)
                                nav.pop()
                            } catch (e: IOException) {
                                nav.message = if (e is ApiException) e.message else "Couldn't reach the server."
                            }
                        }
                    }) { Text("Delete") }
                },
                dismissButton = { TextButton(onClick = { deleting = false }) { Text("Keep it") } },
            )
        }
    }
}

@UnstableApi
@Composable
fun SearchScreen(nav: Nav) {
    var query by rememberSaveable { mutableStateOf("") }
    var results by remember { mutableStateOf<SearchResults?>(null) }
    var failed by remember { mutableStateOf(false) }
    var thinking by remember { mutableStateOf(false) }
    var canWish by remember { mutableStateOf(false) }
    LaunchedEffect(Unit) {
        canWish = try {
            nav.api.wishesEnabled()
        } catch (_: IOException) {
            false
        }
    }
    LaunchedEffect(query) {
        failed = false
        thinking = false
        if (query.isBlank()) {
            results = null
            return@LaunchedEffect
        }
        // Wait for typing to stop.
        delay(250)
        val found = try {
            nav.api.search(query.trim())
        } catch (e: ApiException) {
            failed = true
            return@LaunchedEffect
        } catch (_: IOException) {
            // Away from the server: what's on this phone.
            results = SearchResults(
                tracks = nav.offline.dao.search(nav.server, "%${query.trim()}%").map { it.toTrack() },
            )
            return@LaunchedEffect
        }
        results = found
        if (!found.ask) return@LaunchedEffect
        // Words only the server's AI reader might know: it takes a few
        // seconds, so wait a little longer for typing to stop.
        thinking = true
        try {
            delay(600)
            nav.api.ask(query.trim())?.let { results = found.copy(described = it) }
        } catch (_: IOException) {
            // Keep what was found.
        } finally {
            thinking = false
        }
    }
    Column(Modifier.fillMaxSize()) {
        OutlinedTextField(
            value = query,
            onValueChange = { query = it },
            singleLine = true,
            placeholder = { Text(t("search.placeholder")) },
            leadingIcon = { Icon(Icons.Default.Search, contentDescription = null) },
            modifier = Modifier
                .fillMaxWidth()
                .padding(12.dp),
        )
        if (failed) Note("Couldn't reach the server.")
        val found = results ?: run {
            LibraryScreen(nav)
            return@Column
        }
        val empty = found.tracks.isEmpty() && found.albums.isEmpty() && found.artists.isEmpty() &&
            found.described?.tracks.isNullOrEmpty() && found.lyrics.isEmpty()
        LazyColumn(Modifier.fillMaxSize()) {
            if (thinking) {
                item {
                    LinearProgressIndicator(Modifier.fillMaxWidth())
                    Note(t("go.thinking"))
                }
            }
            if (empty && !thinking) item { Note(t("go.no_songs")) }
            if (found.artists.isNotEmpty()) {
                item { Heading(t("library.artists")) }
                items(found.artists) { artist ->
                    Row2(
                        title = artist.name,
                        subtitle = null,
                        art = { Art(nav.api.artUrl(artist.artHash), 44.dp, CircleShape) },
                        onClick = { nav.push(Screen.Artist(artist.id)) },
                        end = { LikeHeart(artist.id, nav.likedArtists, artist.name, nav.api::setArtistLike, nav) },
                    )
                }
            }
            if (found.albums.isNotEmpty()) {
                item { Heading(t("library.albums")) }
                items(found.albums) { AlbumRow(it, nav) }
            }
            songSection(t("library.songs"), found.tracks, nav)
            if (found.lyrics.isNotEmpty()) {
                item { Heading(t("search.lyric_matches")) }
                val songs = found.lyrics.map { it.track }
                itemsIndexed(found.lyrics) { index, hit ->
                    SongRow(hit.track, nav, note = "\u201c${hit.line}\u201d") { nav.player.play(songs, index) }
                }
            }
            found.described?.let { songSection(it.summary.joinToString(", "), it.tracks, nav) }
            if (canWish) item { AskForSong(query.trim(), nav) }
        }
    }
}

/** "Not in the library?": asks the admin for a song. */
@UnstableApi
@Composable
private fun AskForSong(query: String, nav: Nav) {
    var asking by remember { mutableStateOf(false) }
    var artist by remember { mutableStateOf("") }
    var title by remember { mutableStateOf("") }
    val scope = nav.scope
    TextButton(
        onClick = {
            title = query
            artist = ""
            asking = true
        },
        modifier = Modifier
            .fillMaxWidth()
            .padding(8.dp),
    ) { Text("Not in the library? Ask for it") }
    if (asking) {
        AlertDialog(
            onDismissRequest = { asking = false },
            title = { Text("Ask for a song") },
            text = {
                Column {
                    OutlinedTextField(
                        value = artist,
                        onValueChange = { artist = it.take(200) },
                        label = { Text("Artist") },
                        singleLine = true,
                    )
                    OutlinedTextField(
                        value = title,
                        onValueChange = { title = it.take(200) },
                        label = { Text("Song") },
                        singleLine = true,
                    )
                }
            },
            confirmButton = {
                TextButton(enabled = artist.isNotBlank() && title.isNotBlank(), onClick = {
                    asking = false
                    scope.launch {
                        nav.message = try {
                            nav.api.addWish(artist.trim(), title.trim())
                            "Asked. The admin will see it."
                        } catch (e: IOException) {
                            if (e is ApiException) e.message else "Couldn't reach the server."
                        }
                    }
                }) { Text("Ask") }
            },
            dismissButton = { TextButton(onClick = { asking = false }) { Text("Cancel") } },
        )
    }
}
