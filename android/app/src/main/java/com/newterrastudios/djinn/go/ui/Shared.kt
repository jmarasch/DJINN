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
import kotlinx.coroutines.launch
import androidx.compose.material.icons.filled.FavoriteBorder
import androidx.compose.material.icons.filled.Favorite
import androidx.compose.material.icons.filled.OfflinePin
import androidx.compose.material.icons.filled.Mic
import androidx.compose.material.icons.outlined.MicNone
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
import androidx.compose.foundation.Canvas
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Shadow
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.sp
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import coil3.compose.AsyncImage
import com.newterrastudios.djinn.go.data.Track
import com.newterrastudios.djinn.go.i18n.t

fun duration(ms: Long?): String {
    if (ms == null || ms <= 0) return ""
    val seconds = ms / 1000
    return "%d:%02d".format(seconds / 60, seconds % 60)
}

/** A picture, or a disc until there is one. */
@Composable
fun Art(url: String?, size: Dp, shape: Shape = RoundedCornerShape(10.dp), modifier: Modifier = Modifier) {
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
    titleMark: @Composable () -> Unit = {},
    /** A second line of small print under the subtitle, such as counts. */
    extra: String? = null,
) {
    // Each row is a neon card, like the web app's lists.
    val card = RoundedCornerShape(14.dp)
    Row(
        Modifier
            .fillMaxWidth()
            .padding(horizontal = 12.dp, vertical = 4.dp)
            .neon(card)
            .clip(card)
            .clickable(onClick = onClick)
            .padding(horizontal = 10.dp, vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        art()
        Column(Modifier.weight(1f)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(title, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f, fill = false))
                titleMark()
            }
            if (!subtitle.isNullOrEmpty()) {
                Text(
                    subtitle,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            if (!extra.isNullOrEmpty()) {
                Text(
                    extra,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    style = MaterialTheme.typography.labelMedium,
                    color = MaterialTheme.colorScheme.primary,
                )
            }
        }
        end()
    }
}

/** "12 plays" and "♥ 3" (people who favorited it) for a row's subtitle; none when zero. */
fun countParts(plays: Long, likes: Long): List<String> = listOfNotNull(
    if (plays > 0) "%,d %s".format(plays, if (plays == 1L) "play" else "plays") else null,
    if (likes > 0) "\u2665 %,d".format(likes) else null,
)

/** One song: tap to play, dots for everything else. */
@UnstableApi
@Composable
fun SongRow(track: Track, nav: Nav, note: String? = null, onClick: () -> Unit) {
    val line = (listOfNotNull(note, track.artist, track.album) + countParts(track.plays, track.likes)).joinToString(" · ")
    Row2(
        title = track.title,
        subtitle = line,
        art = { Art(nav.api.artUrl(track.artHash), 44.dp) },
        titleMark = { LyricsMark(track.lyrics) },
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
                    contentDescription = t("kept.on_this_phone"),
                    tint = MaterialTheme.colorScheme.primary,
                    modifier = Modifier.size(18.dp),
                )
            }
            val liked = track.id in nav.liked
            IconButton(onClick = { nav.scope.launch { nav.setLiked(track.id, !liked) } }) {
                Icon(
                    if (liked) Icons.Default.Favorite else Icons.Default.FavoriteBorder,
                    contentDescription = if (liked) t("go.unlike_named", "name" to track.title) else t("go.like_named", "name" to track.title),
                    tint = if (liked) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            IconButton(onClick = { nav.menuFor = track }) {
                Icon(Icons.Default.MoreVert, contentDescription = t("go.more_for", "title" to track.title))
            }
        },
    )
}

/** Beside a song's title: a hollow microphone for lyrics, a filled one when they're timed. */
@Composable
fun LyricsMark(lyrics: Int) {
    if (lyrics <= 0) return
    val timed = lyrics >= 2
    Icon(
        if (timed) Icons.Default.Mic else Icons.Outlined.MicNone,
        contentDescription = t(if (timed) "lyrics.timed_mark" else "lyrics.words_mark"),
        tint = if (timed) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant,
        modifier = Modifier.padding(start = 4.dp).size(14.dp),
    )
}

@Composable
fun Heading(text: String) {
    Text(
        text,
        style = MaterialTheme.typography.titleMedium.copy(
            fontWeight = FontWeight.SemiBold,
            shadow = Shadow(MaterialTheme.colorScheme.primary.copy(alpha = 0.55f), blurRadius = 18f),
        ),
        modifier = Modifier.padding(start = 16.dp, end = 16.dp, top = 16.dp, bottom = 4.dp),
    )
}

@Composable
fun Note(text: String, modifier: Modifier = Modifier) {
    Text(text, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = modifier.padding(16.dp))
}

/** A screen's big title between two small waveforms, as on the web (PageTitle). */
@Composable
fun PageTitle(title: String, subtitle: String? = null) {
    val accent = MaterialTheme.colorScheme.primary
    val heights = listOf(3f, 6f, 10f, 15f, 9f, 13f, 6f, 4f)
    @Composable
    fun Wave(bars: List<Float>) {
        Canvas(Modifier.size(width = 44.dp, height = 22.dp)) {
            bars.forEachIndexed { i, h ->
                val x = (i * 5.5f + 1f).dp.toPx()
                val hh = (h + 2f).dp.toPx()
                drawRoundRect(
                    accent,
                    topLeft = Offset(x, (size.height - hh) / 2),
                    size = Size(2.4f.dp.toPx(), hh),
                    cornerRadius = CornerRadius(1.2f.dp.toPx()),
                )
            }
        }
    }
    Column(Modifier.fillMaxWidth().padding(top = 12.dp, bottom = 8.dp), horizontalAlignment = Alignment.CenterHorizontally) {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            Wave(heights)
            Text(
                title,
                style = MaterialTheme.typography.headlineMedium.copy(
                    fontWeight = FontWeight.Bold,
                    shadow = Shadow(accent.copy(alpha = 0.6f), blurRadius = 24f),
                ),
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.weight(1f, fill = false),
            )
            Wave(heights.reversed())
        }
        if (!subtitle.isNullOrEmpty()) {
            Text(
                subtitle.uppercase(),
                style = MaterialTheme.typography.labelSmall.copy(letterSpacing = 2.sp),
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                textAlign = TextAlign.Center,
            )
        }
    }
}

/** A cover that fills its column, in a neon frame, for the library's picture tiles. */
@Composable
fun TileArt(url: String?, shape: Shape, modifier: Modifier = Modifier) {
    androidx.compose.foundation.layout.BoxWithConstraints(modifier.fillMaxWidth(), contentAlignment = Alignment.Center) {
        val side = this.maxWidth - 2.dp
        Box(Modifier.neon(shape)) {
            Art(url, side, shape, Modifier.padding(1.dp))
        }
    }
}

/** A tile in the library's picture view: the cover, a name, a line of small print, and a heart in the corner. */
@Composable
fun Tile(
    art: @Composable () -> Unit,
    title: String,
    subtitle: String?,
    onClick: () -> Unit,
    corner: @Composable () -> Unit = {},
) {
    Column(
        Modifier
            .fillMaxWidth()
            .padding(horizontal = 8.dp, vertical = 8.dp)
            .clickable(onClick = onClick),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Box(Modifier.fillMaxWidth()) {
            art()
            Box(
                Modifier
                    .align(Alignment.TopEnd)
                    .padding(6.dp)
                    .clip(androidx.compose.foundation.shape.CircleShape)
                    .background(androidx.compose.ui.graphics.Color.Black.copy(alpha = 0.45f)),
            ) { corner() }
        }
        Text(
            title,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            style = MaterialTheme.typography.titleSmall,
            textAlign = TextAlign.Center,
            modifier = Modifier.padding(top = 8.dp),
        )
        if (!subtitle.isNullOrEmpty()) {
            Text(
                subtitle,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                textAlign = TextAlign.Center,
            )
        }
    }
}
