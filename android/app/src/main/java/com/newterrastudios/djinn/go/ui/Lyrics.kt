package com.newterrastudios.djinn.go.ui

import androidx.compose.animation.animateColorAsState
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.positionInParent
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import com.newterrastudios.djinn.go.data.LyricLine
import com.newterrastudios.djinn.go.data.Lyrics
import kotlinx.coroutines.delay

/** The line being sung: the last one started by [ms], or -1 before the first. */
fun lineAt(lines: List<LyricLine>, ms: Long): Int {
    var lo = 0
    var hi = lines.lastIndex
    var found = -1
    while (lo <= hi) {
        val mid = (lo + hi) ushr 1
        if (lines[mid].ms <= ms) {
            found = mid
            lo = mid + 1
        } else {
            hi = mid - 1
        }
    }
    return found
}

/**
 * A song's lyrics under the player. Timed ones follow the music: the line
 * being sung is lit and kept in the middle of the box.
 */
@Composable
fun LyricsBlock(lyrics: Lyrics?, loading: Boolean, position: () -> Long) {
    val muted = MaterialTheme.colorScheme.onSurfaceVariant
    when {
        loading -> Note("Loading lyrics...")
        lyrics == null -> Note("No lyrics for this song.")
        lyrics.instrumental -> Note("Instrumental")
        !lyrics.lines.isNullOrEmpty() -> {
            val lines = lyrics.lines
            var at by remember(lines) { mutableIntStateOf(-1) }
            LaunchedEffect(lines) {
                while (true) {
                    at = lineAt(lines, position())
                    delay(100)
                }
            }
            val scroll = rememberScrollState()
            val tops = remember(lines) { IntArray(lines.size) }
            var boxHeight by remember { mutableIntStateOf(0) }
            LaunchedEffect(at, boxHeight) {
                val line = at.coerceAtLeast(0)
                if (line < tops.size && boxHeight > 0) scroll.animateScrollTo((tops[line] - boxHeight / 2).coerceAtLeast(0))
            }
            Column(
                Modifier
                    .fillMaxWidth()
                    .heightIn(max = 320.dp)
                    .onGloballyPositioned { boxHeight = it.size.height }
                    .verticalScroll(scroll)
                    .padding(vertical = 8.dp),
            ) {
                lines.forEachIndexed { i, line ->
                    val color by animateColorAsState(
                        if (i == at) MaterialTheme.colorScheme.onSurface else muted,
                        label = "lyric",
                    )
                    Text(
                        line.text.ifEmpty { "♪" },
                        color = color,
                        fontWeight = if (i == at) FontWeight.Bold else FontWeight.Normal,
                        style = MaterialTheme.typography.bodyLarge,
                        textAlign = TextAlign.Center,
                        modifier = Modifier
                            .fillMaxWidth()
                            .onGloballyPositioned { tops[i] = it.positionInParent().y.toInt() }
                            .padding(horizontal = 16.dp, vertical = 5.dp),
                    )
                }
            }
        }
        !lyrics.plain.isNullOrBlank() -> Text(
            lyrics.plain,
            color = muted,
            style = MaterialTheme.typography.bodyLarge,
            textAlign = TextAlign.Center,
            modifier = Modifier.fillMaxWidth().padding(16.dp),
        )
        else -> Note("No lyrics for this song.")
    }
}
