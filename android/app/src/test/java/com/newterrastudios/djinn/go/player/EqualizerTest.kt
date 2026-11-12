package com.newterrastudios.djinn.go.player

import androidx.media3.common.C
import androidx.media3.common.audio.AudioProcessor
import androidx.media3.common.util.UnstableApi
import com.newterrastudios.djinn.go.data.parseEqualizer
import java.nio.ByteBuffer
import java.nio.ByteOrder
import kotlin.math.PI
import kotlin.math.abs
import kotlin.math.sin
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

@UnstableApi
class EqualizerTest {
    @After
    fun flat() {
        GoEqualizer.settings = EqSettings()
    }

    /** The loudest sample after running a 1 kHz tone through the equalizer. */
    private fun peakOfTone(settings: EqSettings): Float {
        GoEqualizer.settings = settings
        val processor = EqualizerProcessor()
        processor.configure(AudioProcessor.AudioFormat(48_000, 1, C.ENCODING_PCM_FLOAT))
        processor.flush()
        val frames = 48_000 / 4
        val input = ByteBuffer.allocateDirect(frames * 4).order(ByteOrder.nativeOrder())
        repeat(frames) { input.putFloat(0.25f * sin(2 * PI * 1000 * it / 48_000).toFloat()) }
        input.flip()
        processor.queueInput(input)
        val output = processor.output
        var peak = 0f
        // Past the first moments, once the filters have settled.
        output.position(output.limit() / 2)
        while (output.remaining() >= 4) peak = maxOf(peak, abs(output.getFloat()))
        return peak
    }

    @Test
    fun offPlaysTheSoundAsItIs() {
        assertEquals(0.25f, peakOfTone(EqSettings(enabled = false, bandsDb = List(12) { 12f })), 0.001f)
    }

    @Test
    fun aBoostedBandIsLouder() {
        val boosted = List(12) { if (EQ_BAND_HZ[it] == 1000) 6f else 0f }
        val peak = peakOfTone(EqSettings(enabled = true, bandsDb = boosted))
        assertTrue("6 dB is about twice as loud: $peak", peak in 0.45f..0.55f)
    }

    @Test
    fun peaksAreEasedNotClipped() {
        val loud = EqSettings(enabled = true, bandsDb = List(12) { 12f }, outputGainDb = 6f)
        assertTrue("never past full scale", peakOfTone(loud) <= 1f)
    }

    @Test
    fun savedSettingsComeBack() {
        assertEquals(EqSettings(), parseEqualizer(null))
        assertEquals(EqSettings(), parseEqualizer("nonsense"))
        val bands = List(12) { it - 3f }
        assertEquals(
            EqSettings(enabled = true, bandsDb = bands, outputGainDb = -2f),
            parseEqualizer("1;-2.0;${bands.joinToString(",")}"),
        )
        assertEquals(12f, parseEqualizer("0;0;${List(12) { 40f }.joinToString(",")}").bandsDb[0], 0f)
    }
}
