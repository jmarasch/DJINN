package com.newterrastudios.djinn.go.ui

import android.graphics.Bitmap
import android.graphics.Paint
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.FilterQuality
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.withTransform
import androidx.compose.ui.graphics.nativeCanvas
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.IntSize
import com.newterrastudios.djinn.go.player.SoundReader
import com.newterrastudios.djinn.go.player.WAVE_POINTS
import kotlin.math.PI
import kotlin.math.abs
import kotlin.math.atan2
import kotlin.math.cos
import kotlin.math.exp
import kotlin.math.floor
import kotlin.math.hypot
import kotlin.math.max
import kotlin.math.min
import kotlin.math.roundToInt
import kotlin.math.sin
import kotlin.math.sqrt
import kotlin.random.Random

// The styles the web visualizer added (web/src/lib/nodeVisual.ts), drawn the
// same way here: Spectrogram, Scope, Mountain, VU, Circle, Particles, Terrain
// and Flyover. Change both together.

/** Spectrogram: columns kept (about ten seconds across) and rows of pitch. */
private const val GRAM_COLUMNS = 240
private const val GRAM_ROWS = 96
private const val GRAM_SECONDS = 10f

private const val TERRAIN_COLS = 37
private const val TERRAIN_SEEN = 21
private const val TERRAIN_ROWS = 18
private const val TERRAIN_ROW_SECS = 0.09f
private const val TERRAIN_HEIGHT = 4.5f

private const val FLYOVER_SIZE = 22
private const val FLYOVER_HEIGHT = 4f
private const val FLYOVER_TURN_SECS = 90f

private const val MAX_SPARKS = 1500

/** Dark purple through red and orange to pale yellow, as spectrograms are often drawn. */
private val HEAT = arrayOf(
    intArrayOf(0, 0, 4),
    intArrayOf(40, 11, 84),
    intArrayOf(101, 21, 110),
    intArrayOf(159, 42, 99),
    intArrayOf(212, 72, 66),
    intArrayOf(245, 125, 21),
    intArrayOf(250, 193, 39),
    intArrayOf(252, 255, 164),
)

private fun heat(level: Float): Int {
    val at = level.coerceIn(0f, 1f) * (HEAT.size - 1)
    val i = min(HEAT.size - 2, floor(at).toInt())
    val t = at - i
    val (a, b) = HEAT[i] to HEAT[i + 1]
    fun part(k: Int) = (a[k] + (b[k] - a[k]) * t).roundToInt()
    return (0xFF shl 24) or (part(0) shl 16) or (part(1) shl 8) or part(2)
}

internal class Spark(
    var x: Float,
    var y: Float,
    var vx: Float,
    var vy: Float,
    var life: Float,
    val color: Int,
    val size: Float,
)

/** What some styles keep between frames: pictures with trails, sparks, the terrain's rows. */
internal class VisualMemory {
    private var lastNanos = 0L
    /** Seconds since the last frame, at most a tenth. */
    var elapsed = 0f
        private set
    /** Rainbow colors turn with this (0 when they don't). */
    var turn = 0f

    val gram: IntArray = IntArray(GRAM_COLUMNS * GRAM_ROWS) { 0xFF000004.toInt() }
    val gramPicture: Bitmap = Bitmap.createBitmap(GRAM_COLUMNS, GRAM_ROWS, Bitmap.Config.ARGB_8888)
    var scrolled = 0f

    /** Scope and Particles draw onto this, faded a little each frame for trails. */
    var trails: Bitmap? = null
    val sparks = ArrayList<Spark>()

    var rows = ArrayDeque<FloatArray>()
    var rowSerial = 0
    var rowRoll = 0f
    val ripples = ArrayList<Float>()
    var pillars = FloatArray(0)
    var orbit = 0f

    val path = Path()
    val paint = Paint(Paint.ANTI_ALIAS_FLAG)

    fun tick(nowNanos: Long) {
        elapsed = if (lastNanos == 0L) 0f else ((nowNanos - lastNanos) / 1e9f).coerceIn(0f, 0.1f)
        lastNanos = nowNanos
    }

    fun trailsFor(width: Int, height: Int): Bitmap {
        val w = max(1, width / 2)
        val h = max(1, height / 2)
        val now = trails
        if (now != null && now.width == w && now.height == h) return now
        return Bitmap.createBitmap(w, h, Bitmap.Config.ARGB_8888).also {
            it.eraseColor(android.graphics.Color.BLACK)
            trails = it
        }
    }

    fun beatRipples(beat: Float) {
        if (beat > 0.95f && (ripples.firstOrNull() ?: 1f) > 0.15f) ripples.add(0, 0f)
        for (i in ripples.indices) ripples[i] += elapsed
        ripples.removeAll { it >= 1.6f }
    }
}

/** A rainbow color for `t` (0 to 1), `light` 0 to 100. */
private fun VisualMemory.hue(t: Float, light: Float = 60f, alpha: Float = 1f): Color {
    val turned = t + turn
    return Color.hsl(((turned - floor(turned)) * 360f), 0.9f, (light / 100f).coerceIn(0f, 1f), alpha.coerceIn(0f, 1f))
}

private fun Color.argb(): Int =
    ((alpha * 255).roundToInt() shl 24) or ((red * 255).roundToInt() shl 16) or ((green * 255).roundToInt() shl 8) or (blue * 255).roundToInt()

/** One of the newer styles; false if `style` isn't one of them. */
internal fun DrawScope.drawMore(style: VisualStyle, sound: SoundReader, memory: VisualMemory): Boolean {
    when (style) {
        VisualStyle.Spectrogram -> spectrogram(sound, memory)
        VisualStyle.Scope -> scope(sound, memory)
        VisualStyle.Mountain -> mountain(sound, memory)
        VisualStyle.Vu -> vu(sound, memory)
        VisualStyle.Circle -> circle(sound, memory)
        VisualStyle.Particles -> particles(sound, memory)
        VisualStyle.Terrain -> terrain(sound, memory)
        VisualStyle.Flyover -> flyover(sound, memory)
        else -> return false
    }
    return true
}

/** The bands spread over `rows` rows, low notes first. */
private fun fineRows(bands: FloatArray, rows: Int) = FloatArray(rows) { row ->
    val at = row.toFloat() / (rows - 1) * (bands.size - 1)
    val i = min(bands.size - 2, floor(at).toInt())
    bands[i] + (bands[i + 1] - bands[i]) * (at - i)
}

/** Time across, pitch up, loudness as color, scrolling right to left. */
private fun DrawScope.spectrogram(sound: SoundReader, memory: VisualMemory) {
    memory.scrolled += GRAM_COLUMNS / GRAM_SECONDS * memory.elapsed
    val step = min(GRAM_COLUMNS, floor(memory.scrolled).toInt())
    if (step > 0) {
        memory.scrolled -= step
        val rows = fineRows(sound.bands, GRAM_ROWS)
        val gram = memory.gram
        for (y in 0 until GRAM_ROWS) {
            val start = y * GRAM_COLUMNS
            System.arraycopy(gram, start + step, gram, start, GRAM_COLUMNS - step)
            // Low notes at the bottom.
            val color = heat(rows[GRAM_ROWS - 1 - y])
            for (x in GRAM_COLUMNS - step until GRAM_COLUMNS) gram[start + x] = color
        }
        memory.gramPicture.setPixels(gram, 0, GRAM_COLUMNS, 0, 0, GRAM_COLUMNS, GRAM_ROWS)
    }
    drawImage(
        memory.gramPicture.asImageBitmap(),
        srcOffset = IntOffset.Zero,
        srcSize = IntSize(GRAM_COLUMNS, GRAM_ROWS),
        dstSize = IntSize(size.width.roundToInt(), size.height.roundToInt()),
        filterQuality = FilterQuality.Low,
    )
}

/** Fades the trail picture a little, lets `draw` add to it at half size, and shows it. */
private fun DrawScope.withTrails(memory: VisualMemory, fade: Float, draw: (android.graphics.Canvas, Float) -> Unit) {
    val picture = memory.trailsFor(size.width.roundToInt(), size.height.roundToInt())
    val canvas = android.graphics.Canvas(picture)
    canvas.drawColor(android.graphics.Color.argb((fade * 255).roundToInt(), 0, 0, 0))
    draw(canvas, picture.width / size.width)
    drawImage(
        picture.asImageBitmap(),
        dstSize = IntSize(size.width.roundToInt(), size.height.roundToInt()),
        filterQuality = FilterQuality.Low,
    )
}

/** Left against right, turned so mono stands upright: one line for mono, a cloud for wide stereo. */
private fun DrawScope.scope(sound: SoundReader, memory: VisualMemory) {
    withTrails(memory, 0.18f) { canvas, scale ->
        val w = size.width * scale
        val h = size.height * scale
        val reach = min(w, h) * 0.42f * 1.6f
        val path = android.graphics.Path()
        for (i in 0 until WAVE_POINTS) {
            val l = sound.left[i]
            val r = sound.right[i]
            val x = w / 2 + (l - r) / sqrt(2f) * reach
            val y = h / 2 - (l + r) / sqrt(2f) * reach
            if (i == 0) path.moveTo(x, y) else path.lineTo(x, y)
        }
        val paint = memory.paint
        paint.style = Paint.Style.STROKE
        paint.strokeJoin = Paint.Join.ROUND
        val color = memory.hue(0.75f + sound.slowMid * 0.3f, 65f + sound.beat * 25f)
        // A soft glow under the line.
        paint.color = color.copy(alpha = 0.25f).argb()
        paint.strokeWidth = max(4f, h / 80f)
        canvas.drawPath(path, paint)
        paint.color = color.argb()
        paint.strokeWidth = max(1.5f, h / 300f)
        canvas.drawPath(path, paint)
    }
}

/** The spectrum as a lit range of hills, with its reflection. */
private fun DrawScope.mountain(sound: SoundReader, memory: VisualMemory) {
    val rows = fineRows(sound.bands, GRAM_ROWS).map { it * it }
    val ground = size.height * 0.68f
    val top = ground * 0.92f
    val path = memory.path
    path.reset()
    path.moveTo(0f, ground)
    rows.forEachIndexed { i, v -> path.lineTo(size.width * i / (rows.size - 1), ground - v * top) }
    path.lineTo(size.width, ground)
    path.close()
    val fill = Brush.verticalGradient(
        0f to memory.hue(0.8f + sound.slowMid * 0.2f, 70f),
        0.6f to memory.hue(0.62f + sound.slowBass * 0.15f, 45f),
        1f to memory.hue(0.62f, 20f, 0.6f),
        startY = ground - top,
        endY = ground,
    )
    drawPath(path, fill)
    drawPath(path, Color.White.copy(alpha = 0.5f + sound.beat * 0.5f), style = Stroke(width = 2f))
    // The reflection, upside down, squashed and faint.
    withTransform({ scale(1f, -0.5f, pivot = Offset(0f, ground)) }) {
        drawPath(path, fill, alpha = 0.25f)
    }
}

/** Two needle meters, left and right, swinging as real ones do. */
private fun DrawScope.vu(sound: SoundReader, memory: VisualMemory) {
    val width = size.width
    val height = size.height
    val meterWidth = min(width * 0.42f, height * 1.1f)
    val radius = meterWidth * 0.55f
    val sweep = (PI / 3).toFloat()
    fun at(share: Float) = (-PI / 2).toFloat() - sweep + share * 2 * sweep
    val ink = Color(40, 30, 20)
    val red = Color(200, 30, 30)
    val needle = Color(20, 15, 10)
    for (side in 0..1) {
        val cx = width / 2 + (if (side == 0) -1 else 1) * (meterWidth / 2 + width * 0.02f)
        val cy = height / 2 + radius * 0.45f
        drawRoundRect(
            Color(250, 238, 208).copy(alpha = 0.92f),
            topLeft = Offset(cx - meterWidth / 2, cy - radius * 1.15f),
            size = Size(meterWidth, radius * 1.35f),
            cornerRadius = CornerRadius(radius * 0.08f),
        )
        val line = max(2f, radius * 0.025f)
        val arc = radius * 0.85f
        fun arcPart(from: Float, to: Float, color: Color) = drawArc(
            color,
            startAngle = Math.toDegrees(at(from).toDouble()).toFloat(),
            sweepAngle = Math.toDegrees((at(to) - at(from)).toDouble()).toFloat(),
            useCenter = false,
            topLeft = Offset(cx - arc, cy - arc),
            size = Size(arc * 2, arc * 2),
            style = Stroke(width = line),
        )
        arcPart(0f, 0.75f, ink)
        arcPart(0.75f, 1f, red)
        for (tick in 0..10) {
            val angle = at(tick / 10f)
            drawLine(
                if (tick >= 8) red else ink,
                Offset(cx + cos(angle) * radius * 0.85f, cy + sin(angle) * radius * 0.85f),
                Offset(cx + cos(angle) * radius * 0.95f, cy + sin(angle) * radius * 0.95f),
                strokeWidth = line,
            )
        }
        val paint = memory.paint
        paint.style = Paint.Style.FILL
        paint.color = ink.argb()
        paint.textSize = radius * 0.12f
        paint.textAlign = Paint.Align.CENTER
        drawContext.canvas.nativeCanvas.drawText(if (side == 0) "L" else "R", cx, cy - radius * 0.35f, paint)
        val angle = at(sound.needles[side].coerceIn(0f, 1f))
        drawLine(
            needle,
            Offset(cx, cy),
            Offset(cx + cos(angle) * radius, cy + sin(angle) * radius),
            strokeWidth = max(2f, radius * 0.018f),
            cap = StrokeCap.Round,
        )
        drawCircle(needle, radius * 0.05f, Offset(cx, cy))
    }
}

/** The wave wrapped around a ring that swells with the bass. */
private fun DrawScope.circle(sound: SoundReader, memory: VisualMemory) {
    val cx = size.width / 2
    val cy = size.height / 2
    val small = min(size.width, size.height)
    val base = small * (0.25f + 0.06f * sound.slowBass + 0.03f * sound.beat)
    val reach = small * 0.14f
    memory.orbit = (memory.orbit + memory.elapsed / 8f) % 1000f
    val turn = memory.orbit
    // Around once, then back the same way, so the ends meet.
    val points = WAVE_POINTS * 2
    for ((scale, alpha) in listOf(1f to 1f, 0.82f to 0.45f)) {
        val path = memory.path
        path.reset()
        for (i in 0 until points) {
            val k = if (i < WAVE_POINTS) i else points - 1 - i
            val p = (sound.left[k] + sound.right[k]) / 2
            val angle = 2 * PI.toFloat() * i / points + turn
            val r = (base + p * reach) * scale
            val x = cx + cos(angle) * r
            val y = cy + sin(angle) * r
            if (i == 0) path.moveTo(x, y) else path.lineTo(x, y)
        }
        path.close()
        drawPath(
            path,
            memory.hue(turn * 0.2f + scale * 0.3f, 65f, alpha),
            style = Stroke(width = max(2f, size.height / 220f) * (1f + sound.bass), join = StrokeJoin.Round),
        )
    }
}

/** Sparks thrown out from the middle, a burst on every beat, colored by pitch. */
private fun DrawScope.particles(sound: SoundReader, memory: VisualMemory) {
    withTrails(memory, 0.22f) { canvas, scale ->
        val w = size.width * scale
        val h = size.height * scale
        val small = min(w, h)
        val bands = sound.bands.size
        val count = (sound.bass * 4 + if (sound.beat > 0.95f) 60 else 0).roundToInt()
        var made = 0
        while (made < count && memory.sparks.size < MAX_SPARKS) {
            val band = Random.nextInt(bands)
            val angle = Random.nextFloat() * 2 * PI.toFloat()
            val speed = small * (0.15f + Random.nextFloat() * 0.5f) * (0.5f + sound.bands[band])
            memory.sparks.add(
                Spark(
                    w / 2,
                    h / 2,
                    cos(angle) * speed,
                    sin(angle) * speed,
                    1f,
                    memory.hue(band.toFloat() / bands * 0.8f + 0.55f, 65f).argb(),
                    small * (0.003f + Random.nextFloat() * 0.006f) * (1f + sound.treble),
                ),
            )
            made++
        }
        val elapsed = memory.elapsed
        val paint = memory.paint
        paint.style = Paint.Style.FILL
        val each = memory.sparks.iterator()
        while (each.hasNext()) {
            val spark = each.next()
            spark.x += spark.vx * elapsed
            spark.y += spark.vy * elapsed
            spark.vx *= 1f - 0.6f * elapsed
            spark.vy *= 1f - 0.6f * elapsed
            spark.life -= elapsed * 0.6f
            if (spark.life <= 0f) {
                each.remove()
                continue
            }
            paint.color = spark.color
            paint.alpha = (spark.life * 255).roundToInt().coerceIn(0, 255)
            canvas.drawCircle(spark.x, spark.y, spark.size, paint)
        }
    }
}

/** Fills one face of a pillar, given its corners on screen. */
private fun DrawScope.face(memory: VisualMemory, a: Offset, b: Offset, c: Offset, d: Offset, color: Color) {
    val path = memory.path
    path.reset()
    path.moveTo(a.x, a.y)
    path.lineTo(b.x, b.y)
    path.lineTo(c.x, c.y)
    path.lineTo(d.x, d.y)
    path.close()
    drawPath(path, color)
}

private fun DrawScope.sky(memory: VisualMemory, middle: Float, light: Float) {
    drawRect(
        Brush.verticalGradient(
            0f to Color(6, 4, 14),
            middle to memory.hue(0.72f, light),
            1f to Color(4, 3, 10),
        ),
    )
}

/**
 * A checkerboard of pillars in perspective: bass in the middle, treble at the
 * edges, each moment starting in the front row and rolling away, and beats
 * sending a ripple of light across it.
 */
private fun DrawScope.terrain(sound: SoundReader, memory: VisualMemory) {
    val cols = TERRAIN_COLS
    val center = (cols - 1) / 2f
    val bands = sound.bands
    fun level(c: Int): Float {
        val band = ((abs(c - center) / center) * (bands.size - 1)).roundToInt()
        val v = bands[band]
        return v * v
    }
    memory.rowRoll += memory.elapsed / TERRAIN_ROW_SECS
    while (memory.rowRoll >= 1f || memory.rows.isEmpty()) {
        memory.rowRoll = if (memory.rows.isEmpty()) 0f else memory.rowRoll - 1f
        memory.rows.addFirst(FloatArray(cols) { level(it) })
        memory.rowSerial++
        if (memory.rows.size > TERRAIN_ROWS) memory.rows.removeLast()
    }
    // The front row follows the sound as it is now.
    val front = memory.rows.first()
    for (c in 0 until cols) front[c] = max(front[c] * 0.6f, level(c))
    memory.beatRipples(sound.beat)

    sky(memory, 0.32f, 14f)
    val width = size.width
    val height = size.height
    val horizon = height * 0.34f
    val near = 3f
    val focal = (width * 0.52f * near) / (TERRAIN_SEEN / 2f)
    val camera = ((height * 0.97f - horizon) * (near - 0.44f)) / focal
    fun point(x: Float, y: Float, z: Float) = Offset(width / 2 + x * focal / z, horizon + (camera - y) * focal / z)
    // Far rows first; in each row, the outside in, so nearer sides cover.
    val order = (0 until cols).sortedByDescending { abs(it - center) }
    for (r in memory.rows.size - 1 downTo 0) {
        val z = near + r + memory.rowRoll
        val fog = 1f - (r + memory.rowRoll) / (TERRAIN_ROWS + 1)
        val row = memory.rows[r]
        for (c in order) {
            val v = row[c]
            val x = c - center
            val h = 0.06f + v * TERRAIN_HEIGHT
            val x0 = x - 0.44f
            val x1 = x + 0.44f
            val zf = z - 0.44f
            val zb = z + 0.44f
            val distance = hypot(x, r + memory.rowRoll + 1)
            var glow = 0f
            for (age in memory.ripples) {
                val d = distance - age * 14f
                glow += exp(-(d * d) / 3f) * (1f - age / 1.6f)
            }
            val checker = if ((c + memory.rowSerial - r) % 2 == 0) 1f else 0.7f
            val light = min(85f, (14f + v * 52f + glow * 30f) * fog * checker + 4f)
            val t = 0.74f - (abs(x) / center) * 0.42f
            if (x != 0f) {
                val side = if (x < 0) x1 else x0
                face(memory, point(side, 0f, zf), point(side, 0f, zb), point(side, h, zb), point(side, h, zf), memory.hue(t, light * 0.55f))
            }
            face(memory, point(x0, 0f, zf), point(x1, 0f, zf), point(x1, h, zf), point(x0, h, zf), memory.hue(t, light * 0.8f))
            if (h < camera) {
                face(memory, point(x0, h, zf), point(x1, h, zf), point(x1, h, zb), point(x0, h, zb), memory.hue(t, light))
            }
        }
    }
}

/**
 * A checkerboard of pillars seen from a helicopter slowly circling it: the
 * middle bounces with the bass, further out with higher sounds, each pillar a
 * little its own way, and beats send a ring of light out from the middle.
 */
private fun DrawScope.flyover(sound: SoundReader, memory: VisualMemory) {
    val bands = sound.bands
    val n = FLYOVER_SIZE
    val middle = (n - 1) / 2f
    val reach = hypot(middle, middle)
    val elapsed = memory.elapsed
    val fall = 2.2f * elapsed
    if (memory.pillars.size != n * n) memory.pillars = FloatArray(n * n)
    for (i in 0 until n * n) {
        val x = (i % n) - middle
        val z = (i / n) - middle
        val out = hypot(x, z) / reach
        val wobble = sin(x * 1.7f + z * 0.9f) * 0.08f + cos(z * 1.3f - x * 0.6f) * 0.06f
        val at = (out + wobble).coerceIn(0f, 1f)
        val level = bands[(at * (bands.size - 1)).roundToInt()]
        val lift = 0.75f + 0.25f * sin(i * 12.9898f)
        val target = level * level * lift
        val was = memory.pillars[i]
        memory.pillars[i] = if (target > was) was + (target - was) * 0.6f else max(target, was - fall)
    }
    memory.beatRipples(sound.beat)

    memory.orbit = (memory.orbit + elapsed / FLYOVER_TURN_SECS) % 1f
    val angle = memory.orbit * 2 * PI.toFloat()
    val distance = n * 0.95f
    val lift = n * (0.62f + 0.05f * sound.slowBass)
    val pitch = atan2(lift, distance)
    val sinA = sin(angle)
    val cosA = cos(angle)
    val sinP = sin(pitch)
    val cosP = cos(pitch)
    val camX = -distance * sinA
    val camZ = -distance * cosA
    val width = size.width
    val height = size.height
    val focal = min(width, height * 1.6f) * 0.95f
    fun point(x: Float, y: Float, z: Float): Offset {
        val xr = x * cosA - z * sinA
        val zr = x * sinA + z * cosA + distance
        val yr = y - lift
        val yc = yr * cosP + zr * sinP
        val zc = max(0.1f, -yr * sinP + zr * cosP)
        return Offset(width / 2 + xr * focal / zc, height * 0.52f - yc * focal / zc)
    }
    sky(memory, 0.5f, 9f)
    val far = FloatArray(n * n) { i ->
        val x = (i % n) - middle
        val z = (i / n) - middle
        (x - camX) * (x - camX) + (z - camZ) * (z - camZ)
    }
    val order = (0 until n * n).sortedByDescending { far[it] }
    val farthest = sqrt(far[order[0]])
    for (i in order) {
        val col = i % n
        val row = i / n
        val x = col - middle
        val z = row - middle
        val v = memory.pillars[i]
        val h = 0.05f + v * FLYOVER_HEIGHT
        val x0 = x - 0.45f
        val x1 = x + 0.45f
        val z0 = z - 0.45f
        val z1 = z + 0.45f
        val out = hypot(x, z)
        var glow = 0f
        for (age in memory.ripples) {
            val d = out - age * 12f
            glow += exp(-(d * d) / 2.5f) * (1f - age / 1.6f)
        }
        val fog = 1f - 0.55f * sqrt(far[i]) / farthest
        val checker = if ((col + row) % 2 == 0) 1f else 0.72f
        val light = min(85f, (12f + v * 55f + glow * 32f) * fog * checker + 3f)
        val t = 0.76f - (out / reach) * 0.45f
        val side = memory.hue(t, light * 0.6f)
        val front = memory.hue(t, light * 0.8f)
        // Only the sides that face the camera, then the top.
        if (camX > x1) face(memory, point(x1, 0f, z0), point(x1, 0f, z1), point(x1, h, z1), point(x1, h, z0), side)
        if (camX < x0) face(memory, point(x0, 0f, z0), point(x0, 0f, z1), point(x0, h, z1), point(x0, h, z0), side)
        if (camZ > z1) face(memory, point(x0, 0f, z1), point(x1, 0f, z1), point(x1, h, z1), point(x0, h, z1), front)
        if (camZ < z0) face(memory, point(x0, 0f, z0), point(x1, 0f, z0), point(x1, h, z0), point(x0, h, z0), front)
        if (lift > h) face(memory, point(x0, h, z0), point(x1, h, z0), point(x1, h, z1), point(x0, h, z1), memory.hue(t, light))
    }
}
