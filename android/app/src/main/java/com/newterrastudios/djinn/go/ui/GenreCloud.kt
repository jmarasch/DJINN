package com.newterrastudios.djinn.go.ui

import androidx.activity.compose.BackHandler
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.gestures.detectTransformGestures
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.FitScreen
import androidx.compose.material.icons.filled.Home
import androidx.compose.material.icons.filled.Remove
import androidx.compose.material3.FilledTonalIconButton
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.drawscope.scale
import androidx.compose.ui.graphics.drawscope.translate
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.drawText
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.media3.common.util.UnstableApi
import com.newterrastudios.djinn.go.data.GenreList
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlin.math.min

private const val MAX_ZOOM = 3f
private const val MOVE_MS = 350
val GENRE_TOP_CHOICES = listOf(50, 100, 200, 300)
val GENRE_LINE_CHOICES = listOf("Fewer" to 2, "Normal" to 4, "More" to 8)

private data class Point(val x: Float, val y: Float)

/**
 * The genres as a map you can move around. It opens on the biggest; tapping
 * one centers on it with what's near it and near those, tapping the center
 * opens its page, and the phone's back button goes back a step.
 */
@UnstableApi
@Composable
fun GenreCloud(
    list: GenreList,
    nav: Nav,
    top: Int,
    lines: Int,
    onTop: (Int) -> Unit,
    onLines: (Int) -> Unit,
    modifier: Modifier = Modifier,
) {
    val center = nav.genrePath.lastOrNull()
    var layout by remember { mutableStateOf<CloudLayout?>(null) }
    var from by remember { mutableStateOf(emptyMap<String, Point>()) }
    var origin by remember { mutableStateOf(Point(0f, 0f)) }
    val moved = remember { Animatable(1f) }
    var box by remember { mutableStateOf(IntSize.Zero) }
    var cx by remember { mutableFloatStateOf(0f) }
    var cy by remember { mutableFloatStateOf(0f) }
    var zoom by remember { mutableFloatStateOf(1f) }
    val density = androidx.compose.ui.platform.LocalDensity.current.density
    val measurer = rememberTextMeasurer(cacheSize = 0)
    val textColor = MaterialTheme.colorScheme.onSurface
    val accent = MaterialTheme.colorScheme.primary

    fun fit(l: CloudLayout): Float =
        if (box.width == 0 || box.height == 0) {
            1f
        } else {
            min(1f, min(box.width / density / l.width, box.height / density / l.height))
        }

    fun showAll() {
        val l = layout ?: return
        cx = l.left + l.width / 2
        cy = l.top + l.height / 2
        zoom = fit(l)
    }

    /** Zooms, keeping the point under the given screen position where it is. */
    fun zoomBy(factor: Float, atX: Float = box.width / 2f, atY: Float = box.height / 2f) {
        val l = layout ?: return
        val next = (zoom * factor).coerceIn(fit(l) * 0.9f, MAX_ZOOM)
        val px = cx + (atX - box.width / 2f) / (zoom * density)
        val py = cy + (atY - box.height / 2f) / (zoom * density)
        zoom = next
        cx = (px - (atX - box.width / 2f) / (zoom * density)).coerceIn(l.left, l.left + l.width)
        cy = (py - (atY - box.height / 2f) / (zoom * density)).coerceIn(l.top, l.top + l.height)
    }

    LaunchedEffect(list, center, top) {
        val next = withContext(Dispatchers.Default) { layOutGenres(list, center, top) }
        val previous = layout?.dots.orEmpty().associate { it.name to Point(it.x, it.y) }
        from = previous
        origin = center?.let(previous::get) ?: Point(0f, 0f)
        layout = next
        cx = next.left + next.width / 2
        cy = next.top + next.height / 2
        zoom = if (center != null) fit(next) else 1f
        if (previous.isNotEmpty()) {
            moved.snapTo(0f)
            moved.animateTo(1f, tween(MOVE_MS))
        } else {
            moved.snapTo(1f)
        }
    }

    BackHandler(enabled = nav.genrePath.isNotEmpty()) { nav.genrePath = nav.genrePath.dropLast(1) }

    val current = layout
    val labels = remember(current, textColor) {
        current?.dots?.map { d ->
            measurer.measure(d.name, TextStyle(color = textColor, fontSize = d.font.sp))
        }.orEmpty()
    }

    Column(modifier) {
        Row(
            Modifier
                .fillMaxWidth()
                .horizontalScroll(rememberScrollState())
                .padding(horizontal = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            if (nav.genrePath.isEmpty()) {
                Text(
                    "Top ${current?.dots?.size ?: 0} of ${list.genres.size} genres. Tap one to see what's near it.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(8.dp),
                )
            } else {
                TextButton(onClick = { nav.genrePath = emptyList() }) { Text("Top") }
                nav.genrePath.forEachIndexed { i, name ->
                    Text("/", color = MaterialTheme.colorScheme.onSurfaceVariant)
                    TextButton(
                        enabled = i != nav.genrePath.lastIndex,
                        onClick = { nav.genrePath = nav.genrePath.take(i + 1) },
                    ) { Text(name) }
                }
            }
        }
        Box(
            Modifier
                .weight(1f)
                .fillMaxWidth()
                .onSizeChanged { box = it },
        ) {
            Canvas(
                Modifier
                    .fillMaxSize()
                    .semantics { contentDescription = "Genres and how they relate. The list view has the same genres." }
                    .pointerInput(current, center) {
                        detectTapGestures { tap ->
                            val l = current ?: return@detectTapGestures
                            val x = cx + (tap.x - size.width / 2f) / (zoom * density)
                            val y = cy + (tap.y - size.height / 2f) / (zoom * density)
                            val dot = dotAt(l, x, y) ?: return@detectTapGestures
                            if (dot.name == center) {
                                nav.push(Screen.Genre(dot.name))
                            } else {
                                nav.genrePath = pathAfter(nav.genrePath, dot.name)
                            }
                        }
                    }
                    .pointerInput(current) {
                        detectTransformGestures { centroid, pan, gesture, _ ->
                            zoomBy(gesture, centroid.x, centroid.y)
                            val l = current ?: return@detectTransformGestures
                            cx = (cx - pan.x / (zoom * density)).coerceIn(l.left, l.left + l.width)
                            cy = (cy - pan.y / (zoom * density)).coerceIn(l.top, l.top + l.height)
                        }
                    },
            ) {
                val l = current ?: return@Canvas
                val progress = moved.value
                val places = l.dots.map { d ->
                    val start = from[d.name] ?: origin
                    Offset(
                        (start.x + (d.x - start.x) * progress - cx) * density,
                        (start.y + (d.y - start.y) * progress - cy) * density,
                    )
                }
                translate(size.width / 2, size.height / 2) {
                    scale(zoom, Offset.Zero) {
                        for (line in l.lines) {
                            drawLine(
                                accent.copy(alpha = 0.15f + 0.5f * line.strength),
                                places[line.a],
                                places[line.b],
                                strokeWidth = (1 + 2.5f * line.strength) * density,
                            )
                        }
                        l.dots.forEachIndexed { i, d ->
                            val at = places[i]
                            drawCircle(accent.copy(alpha = if (d.name == center) 1f else 0.85f), d.r * density, at)
                            if (d.name == center) {
                                drawCircle(
                                    textColor,
                                    d.r * density,
                                    at,
                                    style = androidx.compose.ui.graphics.drawscope.Stroke(2 * density),
                                )
                            }
                            labels.getOrNull(i)?.let { text ->
                                drawText(
                                    text,
                                    topLeft = Offset(at.x - text.size.width / 2f, at.y + (d.r + 4) * density),
                                )
                            }
                        }
                    }
                }
            }
            val corner = Modifier.padding(8.dp)
            FilledTonalIconButton(
                onClick = { nav.genrePath = emptyList() },
                enabled = nav.genrePath.isNotEmpty(),
                modifier = corner.align(Alignment.TopStart),
            ) { Icon(Icons.Default.Home, "Back to the top") }
            FilledTonalIconButton(onClick = ::showAll, modifier = corner.align(Alignment.TopEnd)) {
                Icon(Icons.Default.FitScreen, "Fit this view")
            }
            FilledTonalIconButton(onClick = { zoomBy(1.4f) }, modifier = corner.align(Alignment.BottomStart)) {
                Icon(Icons.Default.Add, "Zoom in")
            }
            FilledTonalIconButton(onClick = { zoomBy(1 / 1.4f) }, modifier = corner.align(Alignment.BottomEnd)) {
                Icon(Icons.Default.Remove, "Zoom out")
            }
        }
        Row(
            Modifier
                .fillMaxWidth()
                .horizontalScroll(rememberScrollState())
                .padding(horizontal = 16.dp),
            horizontalArrangement = Arrangement.spacedBy(6.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text("Genres", style = MaterialTheme.typography.bodySmall)
            GENRE_TOP_CHOICES.forEach { choice ->
                FilterChip(selected = top == choice, onClick = { onTop(choice) }, label = { Text("$choice") })
            }
            Text("Lines", style = MaterialTheme.typography.bodySmall, modifier = Modifier.padding(start = 8.dp))
            GENRE_LINE_CHOICES.forEach { (label, links) ->
                FilterChip(selected = lines == links, onClick = { onLines(links) }, label = { Text(label) })
            }
        }
    }
}
