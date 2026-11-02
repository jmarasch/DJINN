package com.newterrastudios.djinn.go.player

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import kotlin.math.PI
import kotlin.math.abs
import kotlin.math.sin

class SpectrumTest {
    private fun bandsFor(hz: Float, level: Float = 0.8f): FloatArray {
        val analyzer = SpectrumAnalyzer()
        repeat(4096) { i -> analyzer.add(level * sin(2 * PI * hz * i / 44100).toFloat(), 44100) }
        return analyzer.bands
    }

    @Test
    fun `silence shows nothing`() {
        val analyzer = SpectrumAnalyzer()
        repeat(4096) { analyzer.add(0f, 44100) }
        assertTrue(analyzer.bands.all { it == 0f })
    }

    @Test
    fun `a higher tone peaks in a higher band`() {
        val low = bandsFor(200f).withIndex().maxBy { it.value }.index
        val high = bandsFor(5000f).withIndex().maxBy { it.value }.index
        assertTrue("$low then $high", high > low + 10)
    }

    @Test
    fun `a loud tone nearly fills its band and quiet ones less`() {
        assertTrue(bandsFor(1000f).max() > 0.9f)
        assertTrue(bandsFor(1000f, 0.05f).max() < bandsFor(1000f).max())
    }

    @Test
    fun `the band holds the tone's own pitch`() {
        // 40 Hz to 22050 Hz over 48 bands: each band is the same ratio wide.
        val ratio = Math.pow(22050.0 / 40.0, 1.0 / BANDS)
        val expected = Math.log(1000.0 / 40.0) / Math.log(ratio)
        val found = bandsFor(1000f).withIndex().maxBy { it.value }.index
        assertEquals(expected, found.toDouble(), 2.0)
        assertTrue(abs(expected - found) < 2)
    }
}
