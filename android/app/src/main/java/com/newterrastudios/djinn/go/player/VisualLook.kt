package com.newterrastudios.djinn.go.player

import kotlinx.serialization.Serializable
import kotlin.math.abs
import kotlin.math.exp

const val COLORS_APP = "app"
const val COLORS_CUSTOM = "custom"
const val COLORS_ROTATE = "rotate"

/** How the visuals look, from Settings. Read each frame, so changes show at once. */
@Serializable
data class VisualLook(
    val colors: String = COLORS_APP,
    /** The colors for quiet and loud, as 0xRRGGBB. */
    val quiet: Int = 0xB48CFF,
    val loud: Int = 0xFFFFFF,
    /** How long RGB rotation takes to go all the way around. */
    val rotateSeconds: Float = 20f,
    val bars: Int = BANDS,
    /** Space between bars, as a share of each bar's room. */
    val gap: Float = 0.25f,
    val rounded: Boolean = true,
    /** Thickness of Wave's line, in dp. */
    val lineWidth: Float = 4f,
    /** 0 reacts instantly; 1 is the smoothest. */
    val calm: Float = 0.25f,
)

/** Where RGB rotation has got to, from 0 up to 1. `position` spreads it across a style. */
fun rotatedHue(seconds: Double, rotateSeconds: Float, position: Float = 0f): Float {
    val turn = seconds / rotateSeconds.coerceAtLeast(1f) + position * ROTATE_SPREAD
    return (turn - kotlin.math.floor(turn)).toFloat()
}

/** A fully bright color as 0xRRGGBB. */
fun hueToRgb(hue: Float, saturation: Float = 0.85f): Int {
    val h = (hue - kotlin.math.floor(hue)) * 6f
    val x = 1f - abs(h % 2f - 1f)
    val (r, g, b) = when (h.toInt()) {
        0 -> Triple(1f, x, 0f)
        1 -> Triple(x, 1f, 0f)
        2 -> Triple(0f, 1f, x)
        3 -> Triple(0f, x, 1f)
        4 -> Triple(x, 0f, 1f)
        else -> Triple(1f, 0f, x)
    }
    fun channel(v: Float) = ((1f - saturation + saturation * v) * 255f + 0.5f).toInt().coerceIn(0, 255)
    return (channel(r) shl 16) or (channel(g) shl 8) or channel(b)
}

/** The bands, averaged down to `count` bars. */
fun binBands(bands: FloatArray, count: Int): FloatArray {
    val bars = count.coerceIn(1, bands.size)
    return FloatArray(bars) { bar ->
        val from = bar * bands.size / bars
        val until = ((bar + 1) * bands.size / bars).coerceAtLeast(from + 1)
        var sum = 0f
        for (i in from until until) sum += bands[i]
        sum / (until - from)
    }
}

/**
 * The sound, slowed down for anything that moves or scales a picture, and how
 * far the pictures that fly have traveled. Multiplying the time by the bass
 * looks the same at first, but then the tiniest wobble in the bass is a leap
 * in position, and more of one the longer it runs. Adding up small steps can't
 * leap, however long it runs.
 */
class VisualMotion {
    var bass = 0f
        private set
    var mid = 0f
        private set
    var treble = 0f
        private set
    var tunnel = 0.0
        private set
    var stars = 0.0
        private set

    fun step(seconds: Float, bassNow: Float, midNow: Float, trebleNow: Float) {
        val by = seconds.coerceIn(0f, MAX_STEP_SECONDS)
        val follow = 1f - exp(-by / SLOW_SECONDS)
        bass += (bassNow - bass) * follow
        mid += (midNow - mid) * follow
        treble += (trebleNow - treble) * follow
        tunnel += by * (0.35 + 0.5 * bass)
        // Only its fraction is used, so it can wrap without a jump.
        stars = (stars + by * (0.05 + 0.03 * bass)) % 1.0
    }

    private companion object {
        const val SLOW_SECONDS = 0.25f
        const val MAX_STEP_SECONDS = 0.1f
    }
}

enum class FramePace { Fine, DrawSmaller, TooSlow }

/**
 * Whether this device keeps up with a picture. There's no asking a graphics
 * chip what it can do, so this watches how it actually does: first it tries a
 * smaller picture, then it gives up on that style. Gaps (the app in the
 * background, a shader being built) don't count against it.
 */
class FrameWatch(
    private val settleNanos: Long = 1_500_000_000L,
    private val windowNanos: Long = 4_000_000_000L,
    private val minFps: Float = 20f,
) {
    private var startedAt = 0L
    private var lastAt = 0L
    private var frames = 0
    private var shrunk = false
    private var watching: Any? = null

    fun frame(nowNanos: Long, style: Any): FramePace {
        if (style != watching || lastAt == 0L || nowNanos - lastAt > GAP_NANOS) {
            watching = style
            startedAt = nowNanos + settleNanos
            frames = 0
        }
        lastAt = nowNanos
        if (nowNanos < startedAt) return FramePace.Fine
        frames++
        val took = nowNanos - startedAt
        if (took < windowNanos) return FramePace.Fine
        val fps = frames * 1_000_000_000f / took
        startedAt = nowNanos + settleNanos
        frames = 0
        return when {
            fps >= minFps -> FramePace.Fine
            !shrunk -> {
                shrunk = true
                FramePace.DrawSmaller
            }
            else -> FramePace.TooSlow
        }
    }

    private companion object {
        const val GAP_NANOS = 1_000_000_000L
    }
}

private const val ROTATE_SPREAD = 0.35f
