package com.newterrastudios.djinn.go.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectDragGestures
import androidx.compose.foundation.layout.Box
import androidx.compose.material.icons.filled.DragHandle
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.material3.minimumInteractiveComponentSize
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.CustomAccessibilityAction
import com.newterrastudios.djinn.go.player.albumId
import com.newterrastudios.djinn.go.player.artistId
import androidx.compose.ui.semantics.customActions
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.zIndex
import kotlin.math.roundToInt
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.TextButton
import androidx.compose.material.icons.filled.Cast
import androidx.compose.material.icons.filled.CastConnected
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.AutoAwesome
import androidx.compose.material.icons.filled.Fullscreen
import androidx.compose.material.icons.filled.Download
import androidx.compose.material.icons.filled.Notifications
import androidx.compose.material.icons.filled.PhoneInTalk
import androidx.compose.material.icons.filled.Alarm
import androidx.compose.material.icons.filled.Celebration
import androidx.compose.material.icons.filled.Flag
import androidx.compose.material.icons.filled.Lyrics
import com.newterrastudios.djinn.go.data.Lyrics
import androidx.compose.material.icons.automirrored.filled.PlaylistAdd
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.ui.platform.LocalContext
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.selection.selectable
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
import com.newterrastudios.djinn.go.data.REPORT_REASONS
import com.newterrastudios.djinn.go.data.SoundKind
import com.newterrastudios.djinn.go.data.Track
import com.newterrastudios.djinn.go.work.DownloadWorker
import com.newterrastudios.djinn.go.i18n.t
import kotlinx.serialization.encodeToString
import kotlinx.coroutines.launch
import java.io.IOException

/** The artist or album under the title. Opens it when it has an id on this server. */
@Composable
private fun SongLink(name: CharSequence?, id: Long?, action: String, open: (Long) -> Unit) {
    if (name.isNullOrEmpty()) return
    Text(
        name.toString(),
        textAlign = TextAlign.Center,
        maxLines = 1,
        overflow = TextOverflow.Ellipsis,
        color = if (id != null) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant,
        modifier = if (id != null) {
            Modifier
                .minimumInteractiveComponentSize()
                .clickable(role = Role.Button, onClickLabel = action) { open(id) }
        } else {
            Modifier
        },
    )
}

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
    var showVisuals by remember { mutableStateOf(nav.prefs.visualsOn) }
    var showLyrics by remember { mutableStateOf(nav.prefs.lyricsOn) }
    var lyrics by remember(id) { mutableStateOf<Lyrics?>(null) }
    var lyricsLoading by remember(id) { mutableStateOf(true) }
    LaunchedEffect(id, showLyrics) {
        if (!showLyrics || id == null) return@LaunchedEffect
        lyricsLoading = true
        lyrics = try {
            nav.api.lyrics(id)
        } catch (_: Exception) {
            null
        }
        lyricsLoading = false
    }
    val context = LocalContext.current
    var pickDevice by remember { mutableStateOf(false) }
    val casting = nav.cast.castingTo
    DisposableEffect(Unit) {
        nav.cast.start()
        onDispose { nav.cast.stopLooking() }
    }
    // While casting the sound plays elsewhere, so there's nothing here to react to.
    val canShow = remember { canShowVisuals(context) } && casting == null
    val visuals = canShow && showVisuals

    LazyColumn(Modifier.fillMaxSize()) {
        item {
            Row(Modifier.fillMaxWidth().padding(4.dp), horizontalArrangement = Arrangement.SpaceBetween) {
                IconButton(onClick = nav::pop) {
                    Icon(Icons.Default.KeyboardArrowDown, contentDescription = t("common.close"))
                }
                Row {
                    if (canShow) {
                        IconButton(onClick = {
                            showVisuals = !showVisuals
                            nav.prefs.visualsOn = showVisuals
                        }) {
                            Icon(
                                Icons.Default.AutoAwesome,
                                contentDescription = if (visuals) t("player.show_cover") else t("player.show_visuals"),
                                tint = if (visuals) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                    }
                    IconButton(onClick = { nav.push(if (visuals) Screen.Visuals else Screen.Cover) }) {
                        Icon(Icons.Default.Fullscreen, contentDescription = t("player.full_screen"))
                    }
                    if (casting != null || nav.cast.devices.isNotEmpty()) {
                        IconButton(onClick = { pickDevice = true }) {
                            Icon(
                                if (casting != null) Icons.Default.CastConnected else Icons.Default.Cast,
                                contentDescription = t("player.cast"),
                            )
                        }
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
                if (visuals) {
                    Visual(
                        nav.currentStyle(),
                        onTap = { nav.push(Screen.Visuals) },
                        onTooSlow = nav::styleTooSlow,
                        modifier = Modifier.size(280.dp),
                    )
                } else {
                    Art(
                        item.mediaMetadata.artworkUri?.toString(),
                        280.dp,
                        modifier = Modifier.clickable { nav.push(Screen.Cover) },
                    )
                }
                if (casting != null) {
                    Text(
                        t("player.playing_on", "device" to casting),
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
                SongLink(item.mediaMetadata.artist, item.artistId, t("player.open_artist")) {
                    nav.goTo(Screen.Artist(it))
                }
                SongLink(item.mediaMetadata.albumTitle, item.albumId, t("player.open_album")) {
                    nav.goTo(Screen.Album(it))
                }
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
                        Icon(Icons.Default.Shuffle, t("player.shuffle"), tint = if (player.shuffle) on else off)
                    }
                    IconButton(onClick = player::previous) {
                        Icon(Icons.Default.SkipPrevious, t("player.previous"), Modifier.size(32.dp))
                    }
                    val playing = if (casting != null) nav.cast.remotePlaying else player.playing
                    FilledIconButton(
                        onClick = { if (casting != null) nav.cast.toggleRemote() else player.toggle() },
                        modifier = Modifier.size(64.dp),
                    ) {
                        Icon(
                            if (playing) Icons.Default.Pause else Icons.Default.PlayArrow,
                            if (playing) t("player.pause") else t("player.play"),
                            Modifier.size(36.dp),
                        )
                    }
                    IconButton(
                        onClick = { if (casting != null) nav.cast.nextRemote() else player.next() },
                        enabled = casting != null || player.upNext.isNotEmpty(),
                    ) {
                        Icon(Icons.Default.SkipNext, t("player.next"), Modifier.size(32.dp))
                    }
                    IconButton(onClick = player::cycleRepeat) {
                        Icon(
                            if (player.repeat == Player.REPEAT_MODE_ONE) Icons.Default.RepeatOne else Icons.Default.Repeat,
                            t("player.repeat"),
                            tint = if (player.repeat == Player.REPEAT_MODE_OFF) off else on,
                        )
                    }
                }
                if (id != null) {
                    val liked = id in nav.liked
                    Row {
                        IconButton(onClick = {
                            scope.launch { nav.setLiked(id, !liked) }
                        }) {
                            Icon(
                                if (liked) Icons.Default.Favorite else Icons.Default.FavoriteBorder,
                                if (liked) t("player.unlike") else t("player.like"),
                                tint = if (liked) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                        IconButton(onClick = {
                            showLyrics = !showLyrics
                            nav.prefs.lyricsOn = showLyrics
                        }) {
                            Icon(
                                Icons.Default.Lyrics,
                                if (showLyrics) t("player.hide_lyrics") else t("player.show_lyrics"),
                                tint = if (showLyrics) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                    }
                }
                player.error?.let { Text(it, color = MaterialTheme.colorScheme.error, textAlign = TextAlign.Center) }
                if (showLyrics && id != null) LyricsBlock(lyrics, lyricsLoading, player::livePositionMs)
            }
        }
        if (player.upNext.isNotEmpty()) {
            item {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Box(Modifier.weight(1f)) { Heading(t("player.up_next")) }
                    TextButton(onClick = { nav.undo = player.clearUpcoming(); nav.message = t("player.queue_cleared") }) {
                        Text(t("player.clear_upcoming"))
                    }
                    TextButton(onClick = { nav.undo = player.clearAll(); nav.message = t("player.queue_cleared") }) {
                        Text(t("player.clear_all"))
                    }
                }
            }
            items(player.upNext, key = { "${it.index}-${it.item.mediaId}" }) { entry ->
                var dragged by remember { mutableStateOf(0f) }
                var rowHeight by remember { mutableIntStateOf(1) }
                val title = entry.item.mediaMetadata.title?.toString() ?: ""
                Box(
                    Modifier
                        .onSizeChanged { rowHeight = it.height.coerceAtLeast(1) }
                        .zIndex(if (dragged != 0f) 1f else 0f)
                        .graphicsLayer { translationY = dragged }
                        .background(if (dragged != 0f) MaterialTheme.colorScheme.surfaceVariant else Color.Transparent),
                ) {
                    Row2(
                        title = title,
                        subtitle = entry.item.mediaMetadata.artist?.toString(),
                        art = { Art(entry.item.mediaMetadata.artworkUri?.toString(), 44.dp) },
                        onClick = { player.jumpTo(entry.index) },
                        end = {
                            IconButton(onClick = { player.remove(entry.index) }) {
                                Icon(Icons.Default.Close, t("player.take_out"))
                            }
                            // The row follows the finger; the queue changes when it's let go.
                            Icon(
                                Icons.Default.DragHandle,
                                contentDescription = t("player.move", "title" to title),
                                tint = MaterialTheme.colorScheme.onSurfaceVariant,
                                modifier = Modifier
                                    .size(40.dp)
                                    .padding(8.dp)
                                    .pointerInput(entry.index) {
                                        detectDragGestures(
                                            onDrag = { change, by ->
                                                change.consume()
                                                dragged += by.y
                                            },
                                            onDragEnd = {
                                                val places = (dragged / rowHeight).roundToInt()
                                                dragged = 0f
                                                if (places != 0) player.move(entry.index, entry.index + places)
                                            },
                                            onDragCancel = { dragged = 0f },
                                        )
                                    }
                                    .semantics {
                                        customActions = listOf(
                                            CustomAccessibilityAction(t("player.move_up")) {
                                                player.move(entry.index, entry.index - 1)
                                                true
                                            },
                                            CustomAccessibilityAction(t("player.move_down")) {
                                                player.move(entry.index, entry.index + 1)
                                                true
                                            },
                                        )
                                    },
                            )
                        },
                    )
                }
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
    var reporting by remember { mutableStateOf(false) }
    LaunchedEffect(track.id) { kept = nav.offline.local(track.id) }
    fun then(action: () -> Unit) {
        nav.menuFor = null
        action()
    }
    fun saveAs(kind: SoundKind) = then {
        scope.launch {
            if (!nav.askStorage()) {
                nav.message = t("menu.sound_permission")
                return@launch
            }
            nav.message = t("menu.saving")
            try {
                val song = nav.offline.local(track.id) ?: nav.offline.fetch(track, KIND_DOWNLOAD)
                nav.offline.saveSound(song, kind)
                nav.message = t("menu.saved_${kind.label}")
            } catch (e: IOException) {
                nav.message = t("menu.save_failed", "error" to (e.message ?: ""))
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
        MenuRow(t("menu.play_next"), Icons.Default.SkipNext) { then { nav.player.playNext(track) } }
        MenuRow(t("menu.add_to_end"), Icons.AutoMirrored.Filled.QueueMusic) { then { nav.player.addToEnd(track) } }
        MenuRow(t("menu.start_radio"), Icons.Default.Radio) { then { nav.radioFor = track } }
        MenuRow(t("menu.add_to_playlist"), Icons.AutoMirrored.Filled.PlaylistAdd) {
            scope.launch {
                choosingPlaylist = try {
                    nav.api.playlists().also { nav.prefs.playlists = nav.api.json.encodeToString(it) }
                } catch (_: IOException) {
                    // Away from the server: the ones last seen, added when it's back.
                    nav.prefs.playlists?.let { nav.api.json.decodeFromString<List<GoPlaylist>>(it) }.orEmpty()
                }
            }
        }
        MenuRow(t("menu.send_to_party"), Icons.Default.Celebration) {
            then {
                scope.launch {
                    nav.message = try {
                        nav.api.sendToParty(track.id)
                        t("menu.sent_to_party")
                    } catch (e: IOException) {
                        if (e is ApiException) e.message else t("common.no_server")
                    }
                }
            }
        }
        if (nav.canKeepSongs) {
            if (kept != null) {
                MenuRow(t("menu.remove_from_phone"), Icons.Default.Delete) {
                    then { scope.launch { nav.offline.remove(track.id) } }
                }
            } else {
                MenuRow(t("menu.download"), Icons.Default.Download) {
                    then {
                        DownloadWorker.enqueue(context, listOf(track), nav.prefs.wifiOnly)
                        nav.message = if (nav.prefs.wifiOnly) t("menu.downloading_wifi") else t("menu.downloading")
                    }
                }
            }
            MenuRow(t("menu.save_alarm"), Icons.Default.Alarm) { saveAs(SoundKind.Alarm) }
            MenuRow(t("menu.save_ringtone"), Icons.Default.PhoneInTalk) { saveAs(SoundKind.Ringtone) }
            MenuRow(t("menu.save_notification"), Icons.Default.Notifications) { saveAs(SoundKind.Notification) }
        }
        val liked = track.id in nav.liked
        MenuRow(if (liked) t("player.unlike") else t("player.like"), if (liked) Icons.Default.Favorite else Icons.Default.FavoriteBorder) {
            then { scope.launch { nav.setLiked(track.id, !liked) } }
        }
        track.albumId?.let { album ->
            MenuRow(t("menu.go_to_album"), Icons.Default.Album) { then { nav.goTo(Screen.Album(album)) } }
        }
        track.artistId?.let { artist ->
            MenuRow(t("menu.go_to_artist"), Icons.Default.Person) { then { nav.goTo(Screen.Artist(artist)) } }
        }
        MenuRow(t("menu.report"), Icons.Default.Flag) { reporting = true }
    }
    if (reporting) {
        ReportDialog(
            title = track.title,
            onDismiss = { reporting = false },
            onSend = { reason, note ->
                reporting = false
                then {
                    scope.launch {
                        nav.message = try {
                            nav.api.reportSong(track.id, reason, note)
                            t("menu.report_thanks")
                        } catch (e: IOException) {
                            if (e is ApiException) e.message else t("common.no_server")
                        }
                    }
                }
            },
        )
    }
    choosingPlaylist?.let { lists ->
        AlertDialog(
            onDismissRequest = { choosingPlaylist = null },
            title = { Text(t("menu.add_to_playlist")) },
            text = {
                Column {
                    if (lists.isEmpty()) Text(t("menu.no_playlists"))
                    lists.forEach { list ->
                        TextButton(onClick = {
                            choosingPlaylist = null
                            then {
                                scope.launch {
                                    nav.message = try {
                                        nav.api.addToPlaylist(list.id, track.id)
                                        t("menu.added_to", "name" to list.name)
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
                                        t("menu.added_offline", "name" to list.name)
                                    }
                                }
                            }
                        }) { Text(list.name) }
                    }
                }
            },
            confirmButton = { TextButton(onClick = { choosingPlaylist = null }) { Text(t("common.cancel")) } },
        )
    }
}

/** Asks what's wrong with a song: a reason and an optional note. */
@Composable
private fun ReportDialog(title: String, onDismiss: () -> Unit, onSend: (String, String?) -> Unit) {
    var reason by remember { mutableStateOf<String?>(null) }
    var note by remember { mutableStateOf("") }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(t("menu.report")) },
        text = {
            Column {
                Text(t("menu.whats_wrong", "title" to title), style = MaterialTheme.typography.bodyMedium)
                REPORT_REASONS.forEach { (id, _) ->
                    Row(
                        verticalAlignment = androidx.compose.ui.Alignment.CenterVertically,
                        modifier = Modifier
                            .fillMaxWidth()
                            .selectable(selected = id == reason, onClick = { reason = id }),
                    ) {
                        androidx.compose.material3.RadioButton(selected = id == reason, onClick = { reason = id })
                        Text(t("report.$id"))
                    }
                }
                androidx.compose.material3.OutlinedTextField(
                    value = note,
                    onValueChange = { note = it.take(300) },
                    label = { Text(t("menu.anything_else")) },
                    modifier = Modifier.fillMaxWidth(),
                )
            }
        },
        confirmButton = {
            TextButton(enabled = reason != null, onClick = {
                reason?.let { onSend(it, note.trim().ifEmpty { null }) }
            }) { Text(t("common.send")) }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text(t("common.cancel")) } },
    )
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
