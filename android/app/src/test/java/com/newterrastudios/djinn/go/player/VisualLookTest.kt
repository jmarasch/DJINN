package com.newterrastudios.djinn.go.player

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import kotlin.math.abs

class VisualLookTest {
    @Test
    fun `flying pictures can't leap however long they've run`() {
        val motion = VisualMotion()
        val frame = 1f / 60
        repeat(60 * 60 * 30) { motion.step(frame, 0.5f, 0f, 0f) }
        val before = motion.tunnel
        // A sudden thump in the bass, half an hour in.
        motion.step(frame, 1f, 0f, 0f)
        val moved = motion.tunnel - before
        assertTrue("moved $moved in one frame", moved < 0.02)

        // What multiplying the time by the bass would have done at that moment.
        val seconds = 60 * 30f
        val leap = seconds * 0.5f * 0.5f
        assertTrue("old way leaps $leap", leap > 400)
    }

    @Test
    fun `slowed sound doesn't follow a single frame's spike`() {
        val motion = VisualMotion()
        motion.step(1f / 60, 1f, 1f, 1f)
        assertTrue(motion.bass < 0.1f)
        repeat(120) { motion.step(1f / 60, 1f, 1f, 1f) }
        assertTrue(motion.bass > 0.95f)
    }

    @Test
    fun `a long gap counts as a short step`() {
        val motion = VisualMotion()
        motion.step(600f, 1f, 0f, 0f)
        assertTrue(motion.tunnel < 0.1)
    }

    @Test
    fun `stars wrap without leaving their fraction`() {
        val motion = VisualMotion()
        repeat(100_000) { motion.step(0.05f, 1f, 0f, 0f) }
        assertTrue(motion.stars in 0.0..1.0)
    }

    @Test
    fun `rotation goes around and starts again`() {
        assertEquals(0f, rotatedHue(0.0, 20f), 0.0001f)
        assertEquals(0.5f, rotatedHue(10.0, 20f), 0.0001f)
        assertEquals(0f, rotatedHue(20.0, 20f), 0.0001f)
        assertEquals(0.25f, rotatedHue(100_005.0, 20f), 0.0001f)
    }

    @Test
    fun `hues make the colors you'd expect`() {
        assertEquals(0xFF0000, hueToRgb(0f, saturation = 1f))
        assertEquals(0x00FF00, hueToRgb(1f / 3, saturation = 1f))
        assertEquals(0x0000FF, hueToRgb(2f / 3, saturation = 1f))
        assertEquals(0xFF0000, hueToRgb(1f, saturation = 1f))
    }

    @Test
    fun `hues change smoothly all the way around`() {
        fun red(rgb: Int) = (rgb shr 16) and 0xFF
        for (i in 0 until 1000) {
            val step = abs(red(hueToRgb(i / 1000f)) - red(hueToRgb((i + 1) / 1000f)))
            assertTrue("jump of $step at $i", step <= 3)
        }
    }

    @Test
    fun `bars average their bands`() {
        val bands = FloatArray(48) { if (it < 24) 1f else 0f }
        val two = binBands(bands, 2)
        assertEquals(1f, two[0], 0.0001f)
        assertEquals(0f, two[1], 0.0001f)
        assertEquals(48, binBands(bands, 48).size)
        assertEquals(1, binBands(bands, 0).size)
        assertEquals(48, binBands(bands, 500).size)
    }

    private fun run(watch: FrameWatch, fps: Int, seconds: Int, from: Long = 1L, style: String = "Nebula"): Pair<List<FramePace>, Long> {
        val step = 1_000_000_000L / fps
        var at = from
        val seen = mutableListOf<FramePace>()
        repeat(fps * seconds) {
            val pace = watch.frame(at, style)
            if (pace != FramePace.Fine) seen += pace
            at += step
        }
        return seen to at
    }

    @Test
    fun `a device that keeps up is left alone`() {
        assertTrue(run(FrameWatch(), 60, 120).first.isEmpty())
        assertTrue(run(FrameWatch(), 24, 120).first.isEmpty())
    }

    @Test
    fun `a slow device draws smaller first and only then gives up`() {
        val seen = run(FrameWatch(), 12, 20).first
        assertEquals(FramePace.DrawSmaller, seen.first())
        assertEquals(FramePace.TooSlow, seen[1])
    }

    @Test
    fun `time in the background isn't slowness`() {
        val watch = FrameWatch()
        var at = run(watch, 60, 3).second
        // Away for a minute, then back at full speed.
        at += 60_000_000_000L
        assertTrue(run(watch, 60, 30, from = at).first.isEmpty())
    }

    @Test
    fun `changing style starts the count again`() {
        val watch = FrameWatch()
        val (_, at) = run(watch, 12, 3, style = "Nebula")
        assertTrue(run(watch, 60, 30, from = at, style = "Stars").first.isEmpty())
    }
}
