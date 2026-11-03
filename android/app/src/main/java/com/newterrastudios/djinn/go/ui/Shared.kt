package com.newterrastudios.djinn.go.ui

import androidx.media3.common.util.UnstableApi
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Album
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.filled.OfflinePin
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import coil3.compose.AsyncImage
import com.newterrastudios.djinn.go.data.Track

fun duration(ms: Long?): String {
    if (ms == null || ms <= 0) return ""
    val seconds = ms / 1000
    return "%d:%02d".format(seconds / 60, seconds % 60)
}

/** A picture, or a disc until there is one. */
@Composable
fun Art(url: String?, size: Dp, shape: Shape = RoundedCornerShape(6.dp), modifier: Modifier = Modifier) {
    Box(
        modifier
            .size(size)
            .clip(shape)
            .background(MaterialTheme.colorScheme.surfaceVariant),
        contentAlignment = Alignment.Center,
    ) {
        Icon(
            Icons.Default.Album,
            contentDescription = null,
            tint = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.size(size / 2),
        )
        if (url != null) {
            AsyncImage(
                model = url,
                contentDescription = null,
                contentScale = ContentScale.Crop,
                modifier = Modifier.size(size),
            )
        }
    }
}

/** A row with a picture, two lines of text, and something at the end. */
@Composable
fun Row2(
    title: String,
    subtitle: String?,
    art: @Composable () -> Unit,
    onClick: () -> Unit,
    end: @Composable () -> Unit = {},
) {
    Row(
        Modifier
            .fillMaxWidth()
            .clickable(onClick = onClick)
            .padding(horizontal = 16.dp, vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        art()
        Column(Modifier.weight(1f)) {
            Text(title, maxLines = 1, overflow = TextOverflow.Ellipsis)
            if (!subtitle.isNullOrEmpty()) {
                Text(
                    subtitle,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
        end()
    }
}

/** One song: tap to play, dots for everything else. */
@UnstableApi
@Composable
fun SongRow(track: Track, nav: Nav, onClick: () -> Unit) {
    val line = listOfNotNull(track.artist, track.album).joinToString(" · ")
    Row2(
        title = track.title,
        subtitle = line,
        art = { Art(nav.api.artUrl(track.artHash), 44.dp) },
        onClick = {
            // With nothing playing a tap always plays.
            when {
                nav.player.current == null || nav.tapAction == "play" -> onClick()
                nav.tapAction == "queue" -> {
                    nav.player.playNext(track)
                    nav.message = "\"${track.title}\" will play next"
                }
                else -> {
                    nav.player.addToEnd(track)
                    nav.message = "\"${track.title}\" added to the end of the queue"
                }
            }
        },
        end = {
            if (track.id in nav.keptIds) {
                Icon(
                    Icons.Default.OfflinePin,
                    contentDescription = "On this phone",
                    tint = MaterialTheme.colorScheme.primary,
                    modifier = Modifier.size(18.dp),
                )
            }
            IconButton(onClick = { nav.menuFor = track }) {
                Icon(Icons.Default.MoreVert, contentDescription = "More for ${track.title}")
            }
        },
    )
}

@Composable
fun Heading(text: String) {
    Text(
        text,
        style = MaterialTheme.typography.titleMedium,
        modifier = Modifier.padding(start = 16.dp, end = 16.dp, top = 16.dp, bottom = 4.dp),
    )
}

@Composable
fun Note(text: String, modifier: Modifier = Modifier) {
    Text(text, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = modifier.padding(16.dp))
}
