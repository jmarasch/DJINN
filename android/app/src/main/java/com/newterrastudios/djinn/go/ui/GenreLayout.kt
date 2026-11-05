package com.newterrastudios.djinn.go.ui

import com.newterrastudios.djinn.go.data.GenreLink
import com.newterrastudios.djinn.go.data.GenreList
import kotlin.math.cos
import kotlin.math.hypot
import kotlin.math.max
import kotlin.math.min
import kotlin.math.sin
import kotlin.math.sqrt

/*
 * Where the genre cloud's dots go: the same steps as web/src/lib/GenreCloud.svelte,
 * so a library looks the same on both. Distances are in dp, before zooming.
 */

private const val STEPS = 320
private const val NEAR = 14
private const val NEAR_THOSE = 5
private const val MOST_IN_VIEW = 50
private const val PAD = 24f

class CloudDot(val name: String, val songs: Long, var x: Float, var y: Float, val r: Float, val font: Float) {
    var vx = 0f
    var vy = 0f
    val labelWidth get() = name.length * font * 0.56f
}

data class CloudLine(val a: Int, val b: Int, val strength: Float)

class CloudLayout(
    val dots: List<CloudDot>,
    val lines: List<CloudLine>,
    val left: Float,
    val top: Float,
    val width: Float,
    val height: Float,
)

/** A genre and what's near it, and near those, strongest first; the genre comes first. */
fun around(links: List<GenreLink>, at: Int): List<Int> {
    fun near(genre: Int) = links
        .filter { it.a == genre || it.b == genre }
        .sortedByDescending { it.strength }
        .map { if (it.a == genre) it.b else it.a }
    val first = near(at).take(NEAR)
    val shown = LinkedHashSet<Int>().apply {
        add(at)
        addAll(first)
    }
    for (genre in first) {
        for (next in near(genre).take(NEAR_THOSE)) {
            if (shown.size >= MOST_IN_VIEW) break
            shown.add(next)
        }
    }
    return shown.toList()
}

/** Where tapping a genre leads: on from here, or back to it if it's already on the way. */
fun pathAfter(path: List<String>, name: String): List<String> {
    val at = path.indexOf(name)
    return if (at >= 0) path.take(at + 1) else path + name
}

/** The dot whose circle or label is at this point of the drawing. */
fun dotAt(layout: CloudLayout, x: Float, y: Float): CloudDot? = layout.dots
    .filter { d ->
        val half = max(d.r, d.labelWidth / 2) + 4
        x in d.x - half..d.x + half && y in d.y - d.r - 2..d.y + d.r + d.font + 6
    }
    .minByOrNull { hypot(it.x - x, it.y - y) }

fun layOutGenres(list: GenreList, center: String?, biggest: Int): CloudLayout {
    val at = if (center == null) -1 else list.genres.indexOfFirst { it.name == center }
    val shown = if (at >= 0) {
        around(list.links, at)
    } else {
        list.genres.indices.sortedByDescending { list.genres[it].songs }.take(biggest)
    }
    val place = shown.withIndex().associate { (i, genre) -> genre to i }
    val most = max(1L, shown.maxOfOrNull { list.genres[it].songs } ?: 1L)
    val dots = shown.mapIndexed { i, genre ->
        val share = sqrt(list.genres[genre].songs.toFloat() / most)
        // Golden-angle spiral, biggest in the middle.
        val angle = i * 2.39996f
        val out = 34 * sqrt(i.toFloat())
        CloudDot(list.genres[genre].name, list.genres[genre].songs, cos(angle) * out, sin(angle) * out, 5 + 15 * share, 12 + 7 * share)
    }
    val lines = (if (at >= 0) list.links else list.topLinks).mapNotNull { link ->
        val a = place[link.a] ?: return@mapNotNull null
        val b = place[link.b] ?: return@mapNotNull null
        CloudLine(a, b, link.strength)
    }
    simulate(dots, lines)
    separate(dots)
    if (dots.isEmpty()) return CloudLayout(dots, lines, -PAD, -PAD, PAD * 2, PAD * 2)
    val left = dots.minOf { it.x - max(it.r, it.labelWidth / 2) }
    val right = dots.maxOf { it.x + max(it.r, it.labelWidth / 2) }
    val top = dots.minOf { it.y - it.r }
    val bottom = dots.maxOf { it.y + it.r + it.font + 6 }
    return CloudLayout(dots, lines, left - PAD, top - PAD, right - left + PAD * 2, bottom - top + PAD * 2)
}

private fun simulate(dots: List<CloudDot>, lines: List<CloudLine>) {
    for (step in 0 until STEPS) {
        val heat = 1 - step.toFloat() / STEPS
        for (i in dots.indices) {
            for (j in i + 1 until dots.size) {
                val (a, b) = dots[i] to dots[j]
                val dist = max(hypot(b.x - a.x, b.y - a.y), 0.01f)
                val dx = (b.x - a.x) / dist
                val dy = (b.y - a.y) / dist
                // Everyone pushes apart, harder when labels would overlap.
                var push = 3200 / (dist * dist)
                val room = a.r + b.r + max(a.labelWidth, b.labelWidth) * 0.6f
                if (dist < room) push += (room - dist) * 0.2f
                a.vx -= dx * push
                a.vy -= dy * push
                b.vx += dx * push
                b.vy += dy * push
            }
        }
        // Related genres pull together, the more related the closer.
        for (line in lines) {
            val (a, b) = dots[line.a] to dots[line.b]
            val dx = b.x - a.x
            val dy = b.y - a.y
            val dist = max(hypot(dx, dy), 0.01f)
            val rest = 170 - 70 * line.strength + a.r + b.r
            val pull = (dist - rest) * 0.012f * (0.4f + line.strength)
            a.vx += dx / dist * pull
            a.vy += dy / dist * pull
            b.vx -= dx / dist * pull
            b.vy -= dy / dist * pull
        }
        for (d in dots) {
            // A gentle pull to the middle; sideways harder, for a tall screen.
            d.vx -= d.x * 0.01f
            d.vy -= d.y * 0.004f
            d.vx *= 0.82f
            d.vy *= 0.82f
            val speed = hypot(d.vx, d.vy)
            val cap = 30 * heat + 1
            val scale = if (speed > cap) cap / speed else 1f
            d.x += d.vx * scale
            d.y += d.vy * scale
        }
    }
}

/** Nudges apart dots whose labels still overlap, each time along the shorter way out. */
private fun separate(dots: List<CloudDot>) {
    for (round in 0 until 200) {
        var moved = false
        for (i in dots.indices) {
            for (j in i + 1 until dots.size) {
                val (a, b) = dots[i] to dots[j]
                val (halfA, halfB) = max(a.r, a.labelWidth / 2) + 4 to max(b.r, b.labelWidth / 2) + 4
                val wide = min(a.x + halfA, b.x + halfB) - max(a.x - halfA, b.x - halfB)
                val tall = min(a.y + a.r + a.font + 6, b.y + b.r + b.font + 6) - max(a.y - a.r - 2, b.y - b.r - 2)
                if (wide <= 0 || tall <= 0) continue
                moved = true
                if (wide < tall) {
                    val step = (wide / 2 + 0.5f) * if (a.x < b.x) -1 else 1
                    a.x += step
                    b.x -= step
                } else {
                    val step = (tall / 2 + 0.5f) * if (a.y < b.y) -1 else 1
                    a.y += step
                    b.y -= step
                }
            }
        }
        if (!moved) break
    }
}
