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
import androidx.compose.foundation.layout.size
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
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.PathEffect
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.Stroke
import com.newterrastudios.djinn.go.data.GenreWeb
import com.newterrastudios.djinn.go.i18n.t
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlin.math.max
import kotlin.math.min

private const val MAX_ZOOM = 3f
private const val MOVE_MS = 350
/** A genre view opens no smaller than this, so names stay readable. */
private const val READABLE = 0.75f
/** "Reaction against" lines, as on the web. */
private val AGAINST_RED = Color(0xFFFF5C7A)

private data class Point(val x: Float, val y: Float)

/**
 * DJINN's genre web as a map you can move around, the same as the web app's.
 * It opens on the families; tapping one puts it in the middle with what it's
 * a kind of above, its kinds below and its links around it, tapping the
 * center opens its page, the trail leads back up, and the phone's back button
 * goes back a step.
 */
@UnstableApi
@Composable
fun GenreCloud(
    web: GenreWeb,
    nav: Nav,
    every: Boolean,
    onEvery: (Boolean) -> Unit,
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
    val teal = MaterialTheme.colorScheme.secondary
    fun lineColor(kind: LineKind) = when (kind) {
        LineKind.TREE -> textColor.copy(alpha = 0.4f)
        LineKind.FROM -> teal
        LineKind.NEAR -> accent
        LineKind.AGAINST -> AGAINST_RED
    }
    fun dash(kind: LineKind, scale: Float) = when (kind) {
        LineKind.NEAR -> PathEffect.dashPathEffect(floatArrayOf(2 * scale, 4 * scale))
        LineKind.AGAINST -> PathEffect.dashPathEffect(floatArrayOf(7 * scale, 4 * scale))
        else -> null
    }

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

    LaunchedEffect(web, center, every) {
        val next = withContext(Dispatchers.Default) { layOutWeb(web, center, every) }
        val previous = layout?.dots.orEmpty().associate { it.name to Point(it.x, it.y) }
        from = previous
        origin = center?.let(previous::get) ?: Point(0f, 0f)
        layout = next
        val wide = fit(next)
        // A genre view that doesn't fit opens on the genre in the middle.
        val middle = next.dots.firstOrNull()?.takeIf { center != null && wide < READABLE }
        cx = middle?.x ?: (next.left + next.width / 2)
        cy = middle?.y ?: (next.top + next.height / 2)
        zoom = if (center != null) max(wide, READABLE) else 1f
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
            if (center == null) {
                Text(
                    t("gcl.families", "n" to (current?.dots?.size ?: 0)),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(8.dp),
                )
            } else {
                // The center's place in the tree, so there's always a way back up.
                TextButton(onClick = { nav.genrePath = emptyList() }) { Text(t("gcl.top")) }
                val up = lineage(web, center)
                up.forEachIndexed { i, name ->
                    Text("›", color = MaterialTheme.colorScheme.onSurfaceVariant)
                    TextButton(
                        enabled = i != up.lastIndex,
                        onClick = { nav.genrePath = pathAfter(nav.genrePath, name) },
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
                    .semantics { contentDescription = t("genres.cloud_description") }
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
                            drawMapLine(
                                places[line.a],
                                places[line.b],
                                l.dots[line.b].r * density,
                                lineColor(line.kind).copy(
                                    alpha = if (line.kind == LineKind.TREE) 0.4f else 0.4f + 0.5f * line.strength,
                                ),
                                (if (line.kind == LineKind.TREE) 1.5f else 1.4f + 2.4f * line.strength) * density,
                                dash(line.kind, density),
                                arrow = line.kind == LineKind.FROM || line.kind == LineKind.AGAINST,
                            )
                        }
                        l.dots.forEachIndexed { i, d ->
                            val at = places[i]
                            val fill = if (d.home) accent else teal
                            drawCircle(
                                fill.copy(alpha = if (d.songs == 0L) 0.3f else if (d.name == center) 1f else 0.85f),
                                d.r * density,
                                at,
                            )
                            if (d.name == center) {
                                drawCircle(textColor, d.r * density, at, style = Stroke(2 * density))
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
            ) { Icon(Icons.Default.Home, t("genres.back_to_top")) }
            FilledTonalIconButton(onClick = ::showAll, modifier = corner.align(Alignment.TopEnd)) {
                Icon(Icons.Default.FitScreen, t("genres.fit"))
            }
            FilledTonalIconButton(onClick = { zoomBy(1.4f) }, modifier = corner.align(Alignment.BottomStart)) {
                Icon(Icons.Default.Add, t("genres.zoom_in"))
            }
            FilledTonalIconButton(onClick = { zoomBy(1 / 1.4f) }, modifier = corner.align(Alignment.BottomEnd)) {
                Icon(Icons.Default.Remove, t("genres.zoom_out"))
            }
        }
        Row(
            Modifier
                .fillMaxWidth()
                .horizontalScroll(rememberScrollState())
                .padding(horizontal = 16.dp),
            horizontalArrangement = Arrangement.spacedBy(10.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            FilterChip(selected = every, onClick = { onEvery(!every) }, label = { Text(t("gcl.every")) })
            listOf(
                LineKind.TREE to "gcl.key_kind",
                LineKind.FROM to "gw.grew_from",
                LineKind.NEAR to "gw.close_to",
                LineKind.AGAINST to "gw.against",
            ).forEach { (kind, label) ->
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Canvas(Modifier.padding(end = 4.dp).size(22.dp, 10.dp)) {
                        drawLine(
                            lineColor(kind),
                            Offset(0f, size.height / 2),
                            Offset(size.width, size.height / 2),
                            strokeWidth = 2 * density,
                            pathEffect = dash(kind, density),
                        )
                    }
                    Text(t(label), style = MaterialTheme.typography.bodySmall)
                }
            }
        }
    }
}

/** A line from [from] to [to], stopping at the edge of [to]'s dot, with an arrow there if wanted. */
private fun DrawScope.drawMapLine(
    from: Offset,
    to: Offset,
    toRadius: Float,
    color: Color,
    width: Float,
    dash: PathEffect?,
    arrow: Boolean,
) {
    val dx = to.x - from.x
    val dy = to.y - from.y
    val dist = kotlin.math.hypot(dx, dy).coerceAtLeast(0.01f)
    val (ux, uy) = dx / dist to dy / dist
    val cut = min(toRadius + 3 * density, dist / 2)
    val end = Offset(to.x - ux * cut, to.y - uy * cut)
    drawLine(color, from, end, strokeWidth = width, pathEffect = dash)
    if (arrow) {
        val size = 6 * density + width
        val back = Offset(end.x - ux * size, end.y - uy * size)
        val head = Path().apply {
            moveTo(end.x, end.y)
            lineTo(back.x - uy * size / 2, back.y + ux * size / 2)
            lineTo(back.x + uy * size / 2, back.y - ux * size / 2)
            close()
        }
        drawPath(head, color.copy(alpha = 1f))
    }
}
