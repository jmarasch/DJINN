package com.newterrastudios.djinn.go.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.TextButton
import androidx.compose.material.icons.filled.Cast
import androidx.compose.material.icons.filled.CastConnected
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.Download
import androidx.compose.material.icons.filled.Notifications
import androidx.compose.material.icons.filled.PhoneInTalk
import androidx.compose.material.icons.filled.Alarm
import androidx.compose.material.icons.filled.Celebration
import androidx.compose.material.icons.automirrored.filled.PlaylistAdd
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.ui.platform.LocalContext
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.QueueMusic
import androidx.compose.material.icons.filled.Album
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Favorite
import androidx.compose.material.icons.filled.FavoriteBorder
import androidx.compose.material.icons.filled.KeyboardArrowDown
import androidx.compose.material.icons.filled.Pause
import androidx.compose.material.icons.filled.Person
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.filled.Radio
import androidx.compose.material.icons.filled.Repeat
import androidx.compose.material.icons.filled.RepeatOne
import androidx.compose.material.icons.filled.Shuffle
import androidx.compose.material.icons.filled.SkipNext
import androidx.compose.material.icons.filled.SkipPrevious
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilledIconButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Slider
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.media3.common.Player
import androidx.media3.common.util.UnstableApi
import com.newterrastudios.djinn.go.data.ApiException
import com.newterrastudios.djinn.go.data.GoPlaylist
import com.newterrastudios.djinn.go.data.KIND_DOWNLOAD
import com.newterrastudios.djinn.go.data.OfflineSong
import com.newterrastudios.djinn.go.data.SoundKind
import com.newterrastudios.djinn.go.data.Track
import com.newterrastudios.djinn.go.work.DownloadWorker
import kotlinx.serialization.encodeToString
import kotlinx.coroutines.launch
import java.io.IOException

@UnstableApi
@Composable
fun NowPlayingScreen(nav: Nav) {
    val player = nav.player
    val item = player.current
    if (item == null) {
        nav.pop()
        return
    }
    val id = item.mediaId.toLongOrNull()
    val scope = rememberCoroutineScope()
    /** Where the thumb is while being dragged. */
    var dragging by remember { mutableStateOf<Float?>(null) }
    var visual by remember { mutableIntStateOf(nav.prefs.visuals) }
    var pickDevice by remember { mutableStateOf(false) }
    val casting = nav.cast.castingTo
    DisposableEffect(Unit) {
        nav.cast.start()
        onDispose { nav.cast.stopLooking() }
    }
    fun nextVisual() {
        visual = (visual + 1) % VisualStyle.entries.size
        nav.prefs.visuals = visual
    }

    LazyColumn(
        Modifier
            .fillMaxSize()
            .safeDrawingPadding(),
    ) {
        item {
            Row(Modifier.fillMaxWidth().padding(4.dp), horizontalArrangement = Arrangement.SpaceBetween) {
                IconButton(onClick = nav::pop) {
                    Icon(Icons.Default.KeyboardArrowDown, contentDescription = "Close")
                }
                if (casting != null || nav.cast.devices.isNotEmpty()) {
                    IconButton(onClick = { pickDevice = true }) {
                        Icon(
                            if (casting != null) Icons.Default.CastConnected else Icons.Default.Cast,
                            contentDescription = "Cast",
                        )
                    }
                }
            }
        }
        item {
            Column(
                Modifier
                    .fillMaxWidth()
                    .padding(horizontal = 24.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                if (VisualStyle.entries[visual] == VisualStyle.Art) {
                    Art(
                        item.mediaMetadata.artworkUri?.toString(),
                        280.dp,
                        modifier = Modifier.clickable(onClick = ::nextVisual),
                    )
                } else {
                    Visualizer(
                        VisualStyle.entries[visual],
                        onTap = ::nextVisual,
                        modifier = Modifier
                            .size(280.dp),
                    )
                }
                if (casting != null) {
                    Text(
                        "Playing on $casting",
                        color = MaterialTheme.colorScheme.primary,
                        modifier = Modifier.padding(top = 12.dp),
                    )
                }
                Text(
                    item.mediaMetadata.title?.toString() ?: "",
                    style = MaterialTheme.typography.titleLarge,
                    textAlign = TextAlign.Center,
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.padding(top = 20.dp),
                )
                Text(
                    listOfNotNull(item.mediaMetadata.artist, item.mediaMetadata.albumTitle).joinToString(" · "),
                    textAlign = TextAlign.Center,
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Slider(
                    value = dragging ?: player.positionMs.toFloat(),
                    onValueChange = { dragging = it },
                    onValueChangeFinished = {
                        dragging?.let { player.seekTo(it.toLong()) }
                        dragging = null
                    },
                    valueRange = 0f..player.durationMs.coerceAtLeast(1).toFloat(),
                    modifier = Modifier.padding(top = 12.dp),
                )
                Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
                    Text(duration((dragging?.toLong() ?: player.positionMs).coerceAtLeast(1000)), style = MaterialTheme.typography.bodySmall)
                    Text(duration(player.durationMs), style = MaterialTheme.typography.bodySmall)
                }
                Row(
                    Modifier
                        .fillMaxWidth()
                        .padding(vertical = 8.dp),
                    horizontalArrangement = Arrangement.SpaceBetween,
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    val on = MaterialTheme.colorScheme.primary
                    val off = MaterialTheme.colorScheme.onSurfaceVariant
                    IconButton(onClick = player::toggleShuffle) {
                        Icon(Icons.Default.Shuffle, "Shuffle", tint = if (player.shuffle) on else off)
                    }
                    IconButton(onClick = player::previous) {
                        Icon(Icons.Default.SkipPrevious, "Previous", Modifier.size(32.dp))
                    }
                    val playing = if (casting != null) nav.cast.remotePlaying else player.playing
                    FilledIconButton(
                        onClick = { if (casting != null) nav.cast.toggleRemote() else player.toggle() },
                        modifier = Modifier.size(64.dp),
                    ) {
                        Icon(
                            if (playing) Icons.Default.Pause else Icons.Default.PlayArrow,
                            if (playing) "Pause" else "Play",
                            Modifier.size(36.dp),
                        )
                    }
                    IconButton(
                        onClick = { if (casting != null) nav.cast.nextRemote() else player.next() },
                        enabled = casting != null || player.upNext.isNotEmpty(),
                    ) {
                        Icon(Icons.Default.SkipNext, "Next", Modifier.size(32.dp))
                    }
                    IconButton(onClick = player::cycleRepeat) {
                        Icon(
                            if (player.repeat == Player.REPEAT_MODE_ONE) Icons.Default.RepeatOne else Icons.Default.Repeat,
                            "Repeat",
                            tint = if (player.repeat == Player.REPEAT_MODE_OFF) off else on,
                        )
                    }
                }
                if (id != null) {
                    val liked = id in nav.liked
                    IconButton(onClick = {
                        scope.launch { nav.setLiked(id, !liked) }
                    }) {
                        Icon(
                            if (liked) Icons.Default.Favorite else Icons.Default.FavoriteBorder,
                            if (liked) "Unlike" else "Like",
                            tint = if (liked) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }
                player.error?.let { Text(it, color = MaterialTheme.colorScheme.error, textAlign = TextAlign.Center) }
            }
        }
        if (player.upNext.isNotEmpty()) {
            item { Heading("Up next") }
            items(player.upNext, key = { "${it.index}-${it.item.mediaId}" }) { entry ->
                Row2(
                    title = entry.item.mediaMetadata.title?.toString() ?: "",
                    subtitle = entry.item.mediaMetadata.artist?.toString(),
                    art = { Art(entry.item.mediaMetadata.artworkUri?.toString(), 44.dp) },
                    onClick = { player.jumpTo(entry.index) },
                    end = {
                        IconButton(onClick = { player.remove(entry.index) }) {
                            Icon(Icons.Default.Close, "Take out of the queue")
                        }
                    },
                )
            }
        }
    }
}

/** Likes change on screen at once, go when they can, and go back if the server says no. */
@UnstableApi
suspend fun Nav.setLiked(id: Long, on: Boolean) {
    if (on) liked.add(id) else liked.remove(id)
    prefs.likedIds = liked.toSet()
    try {
        api.setLike(id, on)
    } catch (_: ApiException) {
        if (on) liked.remove(id) else liked.add(id)
        prefs.likedIds = liked.toSet()
    } catch (_: IOException) {
        offline.dao.putLike(com.newterrastudios.djinn.go.data.PendingLike(server, id, on))
        syncLater()
    }
}

@UnstableApi
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SongMenu(track: Track, nav: Nav) {
    val scope = nav.scope // The menu closes at once; what it started carries on.
    val context = LocalContext.current
    var kept by remember { mutableStateOf<OfflineSong?>(null) }
    var choosingPlaylist by remember { mutableStateOf<List<GoPlaylist>?>(null) }
    LaunchedEffect(track.id) { kept = nav.offline.local(track.id) }
    fun then(action: () -> Unit) {
        nav.menuFor = null
        action()
    }
    fun saveAs(kind: SoundKind) = then {
        scope.launch {
            if (!nav.askStorage()) {
                nav.message = "Saving a sound needs permission to write to storage."
                return@launch
            }
            nav.message = "Saving..."
            try {
                val song = nav.offline.local(track.id) ?: nav.offline.fetch(track, KIND_DOWNLOAD)
                nav.offline.saveSound(song, kind)
                nav.message = "Saved as a ${kind.label}. Choose it in Clock or your phone's sound settings."
            } catch (e: IOException) {
                nav.message = "Couldn't save it: ${e.message}"
            }
        }
    }
    ModalBottomSheet(onDismissRequest = { nav.menuFor = null }) {
        Text(
            track.title,
            style = MaterialTheme.typography.titleMedium,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp),
        )
        MenuRow("Play next", Icons.Default.SkipNext) { then { nav.player.playNext(track) } }
        MenuRow("Add to queue", Icons.AutoMirrored.Filled.QueueMusic) { then { nav.player.addToQueue(track) } }
        MenuRow("Start radio", Icons.Default.Radio) { then { nav.radioFor = track } }
        MenuRow("Add to playlist", Icons.AutoMirrored.Filled.PlaylistAdd) {
            scope.launch {
                choosingPlaylist = try {
                    nav.api.playlists().also { nav.prefs.playlists = nav.api.json.encodeToString(it) }
                } catch (_: IOException) {
                    // Away from the server: the ones last seen, added when it's back.
                    nav.prefs.playlists?.let { nav.api.json.decodeFromString<List<GoPlaylist>>(it) }.orEmpty()
                }
            }
        }
        MenuRow("Send to the party", Icons.Default.Celebration) {
            then {
                scope.launch {
                    nav.message = try {
                        nav.api.sendToParty(track.id)
                        "Sent to the party."
                    } catch (e: IOException) {
                        if (e is ApiException) e.message else "Couldn't reach the server."
                    }
                }
            }
        }
        if (nav.canKeepSongs) {
            if (kept != null) {
                MenuRow("Remove from this phone", Icons.Default.Delete) {
                    then { scope.launch { nav.offline.remove(track.id) } }
                }
            } else {
                MenuRow("Download", Icons.Default.Download) {
                    then {
                        DownloadWorker.enqueue(context, listOf(track), nav.prefs.wifiOnly)
                        nav.message = "Downloading" + if (nav.prefs.wifiOnly) " when on Wi-Fi" else ""
                    }
                }
            }
            MenuRow("Save as alarm sound", Icons.Default.Alarm) { saveAs(SoundKind.Alarm) }
            MenuRow("Save as ringtone", Icons.Default.PhoneInTalk) { saveAs(SoundKind.Ringtone) }
            MenuRow("Save as notification sound", Icons.Default.Notifications) { saveAs(SoundKind.Notification) }
        }
        val liked = track.id in nav.liked
        MenuRow(if (liked) "Unlike" else "Like", if (liked) Icons.Default.Favorite else Icons.Default.FavoriteBorder) {
            then { scope.launch { nav.setLiked(track.id, !liked) } }
        }
        track.albumId?.let { album ->
            MenuRow("Go to album", Icons.Default.Album) { then { nav.goTo(Screen.Album(album)) } }
        }
        track.artistId?.let { artist ->
            MenuRow("Go to artist", Icons.Default.Person) { then { nav.goTo(Screen.Artist(artist)) } }
        }
    }
    choosingPlaylist?.let { lists ->
        AlertDialog(
            onDismissRequest = { choosingPlaylist = null },
            title = { Text("Add to playlist") },
            text = {
                Column {
                    if (lists.isEmpty()) Text("No playlists here yet. Make one under Library > Lists.")
                    lists.forEach { list ->
                        TextButton(onClick = {
                            choosingPlaylist = null
                            then {
                                scope.launch {
                                    nav.message = try {
                                        nav.api.addToPlaylist(list.id, track.id)
                                        "Added to ${list.name}."
                                    } catch (e: ApiException) {
                                        e.message
                                    } catch (_: IOException) {
                                        nav.offline.dao.addToPlaylist(
                                            com.newterrastudios.djinn.go.data.PendingAdd(
                                                server = nav.server,
                                                playlistId = list.id,
                                                trackId = track.id,
                                            ),
                                        )
                                        nav.syncLater()
                                        "Added to ${list.name}. It reaches the server when you're back online."
                                    }
                                }
                            }
                        }) { Text(list.name) }
                    }
                }
            },
            confirmButton = { TextButton(onClick = { choosingPlaylist = null }) { Text("Cancel") } },
        )
    }
}

@Composable
private fun MenuRow(label: String, icon: ImageVector, onClick: () -> Unit) {
    androidx.compose.material3.TextButton(onClick = onClick, modifier = Modifier.fillMaxWidth()) {
        Icon(icon, contentDescription = null)
        Text(label, modifier = Modifier
            .weight(1f)
            .padding(start = 16.dp))
    }
}
