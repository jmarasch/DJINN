package com.newterrastudios.djinn.go.ui

import com.newterrastudios.djinn.go.data.GenreWeb
import kotlin.math.PI
import kotlin.math.cos
import kotlin.math.hypot
import kotlin.math.max
import kotlin.math.min
import kotlin.math.sin
import kotlin.math.sqrt

/*
 * Where the genre map's dots go: the same steps as web/src/lib/GenreCloud.svelte,
 * so a library looks the same on both. Distances are in dp, before zooming.
 */

private const val STEPS = 320
private const val PAD = 24f
/** At the top, each family's strongest tie to another family is drawn... */
private const val FAMILY_TIES = 1
/** ...and the strongest ties overall. */
private const val TOP_TIES = 12

/** How two genres on the map are joined. */
enum class LineKind { TREE, FROM, NEAR, AGAINST }

class CloudDot(
    /** The genre's place in [GenreWeb.genres]. */
    val id: Int,
    val name: String,
    val songs: Long,
    var x: Float,
    var y: Float,
    val r: Float,
    val font: Float,
    /** In the same family as the center (or a family, at the top). */
    val home: Boolean,
) {
    /** Where it started, which it's drawn back toward. */
    val hx = x
    val hy = y
    var vx = 0f
    var vy = 0f
    val labelWidth get() = name.length * font * 0.56f
}

/** A line from dot [a] to dot [b]; an arrow, if any, points at [b]. */
data class CloudLine(val a: Int, val b: Int, val kind: LineKind, val strength: Float)

class CloudLayout(
    val dots: List<CloudDot>,
    val lines: List<CloudLine>,
    val left: Float,
    val top: Float,
    val width: Float,
    val height: Float,
)

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

/** The family a genre is in. */
fun familyOf(web: GenreWeb, genre: Int): Int {
    var at = genre
    while (true) at = web.genres[at].parent ?: return at
}

/** A genre's place in the tree, from its family down to it. */
fun lineage(web: GenreWeb, name: String): List<String> {
    var at: Int? = web.genres.indexOfFirst { it.name == name }.takeIf { it >= 0 } ?: return emptyList()
    val up = ArrayDeque<String>()
    while (at != null) {
        up.addFirst(web.genres[at].name)
        at = web.genres[at].parent
    }
    return up.toList()
}

private fun dot(web: GenreWeb, id: Int, x: Float, y: Float, most: Long, home: Boolean, big: Boolean = false): CloudDot {
    val genre = web.genres[id]
    val share = min(1f, sqrt(genre.songsBelow.toFloat() / most))
    return if (big) {
        CloudDot(id, genre.name, genre.songsBelow, x, y, 22f, 17f, home)
    } else {
        CloudDot(id, genre.name, genre.songsBelow, x, y, 5 + 14 * share, 12 + 5 * share, home)
    }
}

/** Places around an arc, from [from] to [to] (radians, 0 is right, y down). */
private fun arc(count: Int, radius: Float, from: Double, to: Double): List<Pair<Float, Float>> =
    List(count) { i ->
        val a = if (count == 1) (from + to) / 2 else from + (to - from) * i / (count - 1)
        // Long rows step out and in, so neighbors' labels don't meet.
        val r = radius + if (count > 9 && i % 2 == 1) 70 else 0
        (cos(a) * r).toFloat() to (sin(a) * r).toFloat()
    }

/**
 * The map with [center] in the middle (what it's a kind of above, its kinds
 * below, its links around it), or the families when it's null. Genres none
 * of the library's songs land on are left out unless [every].
 */
fun layOutWeb(web: GenreWeb, center: String?, every: Boolean): CloudLayout {
    val genres = web.genres
    fun shows(i: Int) = every || genres[i].songsBelow > 0
    val at = if (center == null) -1 else genres.indexOfFirst { it.name == center }
    val dots = mutableListOf<CloudDot>()
    val lines = mutableListOf<CloudLine>()
    if (at < 0) {
        val families = genres.indices.filter { genres[it].parent == null && shows(it) }
            .sortedByDescending { genres[it].songsBelow }
        val most = max(1L, families.maxOfOrNull { genres[it].songsBelow } ?: 1L)
        families.forEachIndexed { i, f ->
            // Golden-angle spiral, biggest in the middle.
            val angle = i * 2.39996f
            val out = 60 * sqrt(i.toFloat())
            dots += dot(web, f, cos(angle) * out, sin(angle) * out, most, true)
        }
        val place = families.withIndex().associate { (i, f) -> f to i }

        class Tie(val a: Int, val b: Int) {
            val kinds = mutableMapOf<LineKind, Int>()
            var total = 0
        }
        val pairs = linkedMapOf<String, Tie>()
        genres.forEachIndexed { i, g ->
            fun add(other: Int, kind: LineKind) {
                val fa = place[familyOf(web, i)] ?: return
                val fb = place[familyOf(web, other)] ?: return
                if (fa == fb) return
                // "Grew out of" runs from the older family to the newer one.
                val (a, b) = if (kind == LineKind.FROM) fb to fa else fa to fb
                val pair = pairs.getOrPut("${min(a, b)}-${max(a, b)}") { Tie(a, b) }
                pair.kinds[kind] = (pair.kinds[kind] ?: 0) + 1
                pair.total++
            }
            g.from.forEach { add(it, LineKind.FROM) }
            g.near.forEach { add(it, LineKind.NEAR) }
            g.against.forEach { add(it, LineKind.AGAINST) }
        }
        // Each family's strongest ties, so the overview stays readable.
        val all = pairs.values.sortedByDescending { it.total }
        val strongest = max(1, all.firstOrNull()?.total ?: 1)
        val kept = LinkedHashSet(all.take(TOP_TIES))
        families.indices.forEach { f -> kept += all.filter { it.a == f || it.b == f }.take(FAMILY_TIES) }
        for (pair in kept) {
            val kind = pair.kinds.maxByOrNull { it.value }?.key ?: LineKind.NEAR
            lines += CloudLine(pair.a, pair.b, kind, sqrt(pair.total.toFloat() / strongest))
        }
    } else {
        val family = familyOf(web, at)
        val most = max(1L, genres.filter { it.parent == at }.maxOfOrNull { it.songsBelow } ?: 1L)
        dots += dot(web, at, 0f, 0f, most, home = true, big = true)
        genres[at].parent?.let { dots += dot(web, it, 0f, -200f, most, true) }
        val kids = genres.indices.filter { genres[it].parent == at && shows(it) }
        arc(kids.size, 210f, PI * 0.12, PI * 0.88).forEachIndexed { i, (x, y) ->
            dots += dot(web, kids[i], x, y, most, true)
        }
        val shown = dots.map { it.id }.toMutableSet()
        val linked = mutableListOf<Int>()
        val g = genres[at]
        val touching = genres.indices.filter { at in genres[it].from || at in genres[it].near || at in genres[it].against }
        for (x in g.from + g.near + g.against + touching) {
            if (x !in shown && x !in linked && shows(x)) linked += x
        }
        // Around the top half, either side of the parent.
        val left = linked.filterIndexed { i, _ -> i % 2 == 0 }
        val right = linked.filterIndexed { i, _ -> i % 2 == 1 }
        arc(left.size, 270f, PI * 0.95, PI * 1.38).forEachIndexed { i, (x, y) ->
            dots += dot(web, left[i], x, y, most, familyOf(web, left[i]) == family)
        }
        arc(right.size, 270f, PI * 1.62, PI * 2.05).forEachIndexed { i, (x, y) ->
            dots += dot(web, right[i], x, y, most, familyOf(web, right[i]) == family)
        }
        // Only the lines to and from the genre in the middle (dot 0).
        val index = dots.withIndex().associate { (i, d) -> d.id to i }
        fun add(a: Int, b: Int, kind: LineKind) {
            if (a == 0 || b == 0) lines += CloudLine(a, b, kind, 1f)
        }
        dots.forEachIndexed { i, d ->
            val h = genres[d.id]
            h.parent?.let { p -> index[p]?.let { add(it, i, LineKind.TREE) } }
            h.from.forEach { x -> index[x]?.let { add(it, i, LineKind.FROM) } }
            h.near.forEach { x -> index[x]?.let { add(i, it, LineKind.NEAR) } }
            h.against.forEach { x -> index[x]?.let { add(i, it, LineKind.AGAINST) } }
        }
    }
    simulate(dots, lines, at >= 0)
    separate(dots, at >= 0)
    if (dots.isEmpty()) return CloudLayout(dots, lines, -PAD, -PAD, PAD * 2, PAD * 2)
    val left = dots.minOf { it.x - max(it.r, it.labelWidth / 2) }
    val right = dots.maxOf { it.x + max(it.r, it.labelWidth / 2) }
    val top = dots.minOf { it.y - it.r }
    val bottom = dots.maxOf { it.y + it.r + it.font + 6 }
    return CloudLayout(dots, lines, left - PAD, top - PAD, right - left + PAD * 2, bottom - top + PAD * 2)
}

private fun simulate(dots: List<CloudDot>, lines: List<CloudLine>, centered: Boolean) {
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
        // Linked genres pull together a little; each dot is drawn back toward
        // its starting place, so the shape (parent above, kinds below) holds.
        for (line in lines) {
            val (a, b) = dots[line.a] to dots[line.b]
            val dx = b.x - a.x
            val dy = b.y - a.y
            val dist = max(hypot(dx, dy), 0.01f)
            val rest = 190 + a.r + b.r
            val pull = (dist - rest) * 0.004f * (0.4f + line.strength)
            a.vx += dx / dist * pull
            a.vy += dy / dist * pull
            b.vx -= dx / dist * pull
            b.vy -= dy / dist * pull
        }
        val home = if (centered) 0.08f else 0.03f
        dots.forEachIndexed { i, d ->
            if (centered && i == 0) {
                // The genre in the middle stays there.
                d.vx = 0f
                d.vy = 0f
                return@forEachIndexed
            }
            d.vx += (d.hx - d.x) * home
            d.vy += (d.hy - d.y) * home
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
private fun separate(dots: List<CloudDot>, pinFirst: Boolean) {
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
                // The genre in the middle stays put; the other one moves the whole way.
                val (mine, theirs) = if (pinFirst && i == 0) 0f to 2f else 1f to 1f
                if (wide < tall) {
                    val step = (wide / 2 + 0.5f) * if (a.x < b.x) -1 else 1
                    a.x += step * mine
                    b.x -= step * theirs
                } else {
                    val step = (tall / 2 + 0.5f) * if (a.y < b.y) -1 else 1
                    a.y += step * mine
                    b.y -= step * theirs
                }
            }
        }
        if (!moved) break
    }
}
