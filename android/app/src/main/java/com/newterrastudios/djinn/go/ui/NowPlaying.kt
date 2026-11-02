package com.newterrastudios.djinn.go.ui

import androidx.compose.foundation.layout.Arrangement
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
import com.newterrastudios.djinn.go.data.Track
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

    LazyColumn(
        Modifier
            .fillMaxSize()
            .safeDrawingPadding(),
    ) {
        item {
            IconButton(onClick = nav::pop, modifier = Modifier.padding(4.dp)) {
                Icon(Icons.Default.KeyboardArrowDown, contentDescription = "Close")
            }
        }
        item {
            Column(
                Modifier
                    .fillMaxWidth()
                    .padding(horizontal = 24.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                Art(item.mediaMetadata.artworkUri?.toString(), 280.dp)
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
                    FilledIconButton(onClick = player::toggle, modifier = Modifier.size(64.dp)) {
                        Icon(
                            if (player.playing) Icons.Default.Pause else Icons.Default.PlayArrow,
                            if (player.playing) "Pause" else "Play",
                            Modifier.size(36.dp),
                        )
                    }
                    IconButton(onClick = player::next, enabled = player.upNext.isNotEmpty()) {
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

/** Likes change on screen at once, and go back if the server says no. */
@UnstableApi
suspend fun Nav.setLiked(id: Long, on: Boolean) {
    if (on) liked.add(id) else liked.remove(id)
    try {
        api.setLike(id, on)
    } catch (_: IOException) {
        if (on) liked.remove(id) else liked.add(id)
    }
}

@UnstableApi
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SongMenu(track: Track, nav: Nav) {
    val scope = rememberCoroutineScope()
    fun then(action: () -> Unit) {
        nav.menuFor = null
        action()
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
