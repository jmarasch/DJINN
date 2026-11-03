package com.newterrastudios.djinn.go.player

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import kotlin.math.PI
import kotlin.math.abs
import kotlin.math.sin

class SoundReaderTest {
    private val rate = 44100

    private fun feed(analyzer: SpectrumAnalyzer, seconds: Float, sample: (Int) -> Float) {
        repeat((seconds * rate).toInt()) { analyzer.add(sample(it), rate) }
    }

    /** A low thump four times a second, quiet in between. */
    private fun thumps(i: Int): Float {
        val inBeat = i % (rate / 4)
        val level = if (inBeat < rate / 16) 0.9f else 0.0005f
        return level * sin(2 * PI * 80 * i / rate).toFloat()
    }

    @Test
    fun `bass, mid and treble follow where the sound is`() {
        val low = SpectrumAnalyzer()
        feed(low, 0.2f) { 0.8f * sin(2 * PI * 80 * it / rate).toFloat() }
        val high = SpectrumAnalyzer()
        feed(high, 0.2f) { 0.8f * sin(2 * PI * 8000 * it / rate).toFloat() }
        assertTrue(low.latest()!!.bass > low.latest()!!.treble)
        assertTrue(high.latest()!!.treble > high.latest()!!.bass)
    }

    @Test
    fun `the waveform holds the sound itself`() {
        val analyzer = SpectrumAnalyzer()
        feed(analyzer, 0.1f) { 0.5f * sin(2 * PI * 200 * it / rate).toFloat() }
        val wave = analyzer.latest()!!.wave
        assertTrue(wave.max() > 0.4f && wave.max() <= 0.5f)
        assertTrue(wave.min() < -0.4f && wave.min() >= -0.5f)
    }

    @Test
    fun `thumps count as beats at about their real rate`() {
        val analyzer = SpectrumAnalyzer()
        var beats = 0
        var last = -1L
        var i = 0
        // Four thumps a second for five seconds.
        while (i < rate * 5) {
            analyzer.add(thumps(i), rate)
            val frame = analyzer.latest()
            if (frame != null && frame.atUs != last) {
                last = frame.atUs
                if (frame.beat) beats++
            }
            i++
        }
        assertTrue("$beats beats", beats in 16..22)
    }

    @Test
    fun `a steady tone isn't a beat after it starts`() {
        val analyzer = SpectrumAnalyzer()
        var beats = 0
        var last = -1L
        var i = 0
        while (i < rate * 3) {
            analyzer.add(0.7f * sin(2 * PI * 80 * i / rate).toFloat(), rate)
            val frame = analyzer.latest()
            if (frame != null && frame.atUs != last) {
                last = frame.atUs
                if (i > rate && frame.beat) beats++
            }
            i++
        }
        assertEquals(0, beats)
    }

    @Test
    fun `reading looks back by the lead`() {
        val analyzer = SpectrumAnalyzer()
        feed(analyzer, 1f) { 0f }
        val newest = analyzer.latest()!!.atUs
        val frame = analyzer.at(newest - 250_000)!!
        assertTrue(abs(newest - 250_000 - frame.atUs) < 30_000)
    }

    @Test
    fun `bursts of audio don't make the clock jump`() {
        val analyzer = SpectrumAnalyzer()
        val reader = SoundReader(analyzer)
        var now = 1_000_000_000L
        var fed = 0
        var previous = -1f
        // Audio arrives in 200 ms bursts while frames draw at 60 a second:
        // one loud half second, a quiet half second, so the level has to move.
        var biggestJump = 0f
        repeat(240) { frame ->
            if (frame % 12 == 0) {
                repeat(rate / 5) {
                    val loud = (fed / (rate / 2)) % 2 == 0
                    analyzer.add((if (loud) 0.8f else 0.001f) * sin(2 * PI * 1000 * fed / rate).toFloat(), rate)
                    fed++
                }
            }
            now += 16_666_667L
            reader.read(now, 100)
            val level = reader.bands.max()
            if (previous >= 0 && level < previous) biggestJump = maxOf(biggestJump, previous - level)
            previous = level
        }
        assertTrue("falls smoothly, biggest drop $biggestJump", biggestJump < 0.08f)
    }

    @Test
    fun `when the sound stops, the visuals settle`() {
        val analyzer = SpectrumAnalyzer()
        val reader = SoundReader(analyzer)
        var now = 1_000_000_000L
        feed(analyzer, 1f) { 0.8f * sin(2 * PI * 1000 * it / rate).toFloat() }
        reader.read(now, 0)
        assertTrue(reader.bands.max() > 0.5f)
        repeat(120) {
            now += 16_666_667L
            reader.read(now, 0)
        }
        assertEquals(0f, reader.bands.max(), 0.001f)
        assertEquals(0f, reader.bass, 0.001f)
    }

    @Test
    fun `intensity scales what shows`() {
        val analyzer = SpectrumAnalyzer()
        feed(analyzer, 1f) { 0.5f * sin(2 * PI * 1000 * it / rate).toFloat() }
        val full = SoundReader(analyzer).apply { read(1_000_000_000L, 0, 1f) }.bands.max()
        val half = SoundReader(analyzer).apply { read(1_000_000_000L, 0, 0.5f) }.bands.max()
        assertEquals(full / 2, half, 0.02f)
    }
}
