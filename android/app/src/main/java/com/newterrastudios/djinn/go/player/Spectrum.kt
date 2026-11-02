package com.newterrastudios.djinn.go.player

import androidx.media3.common.C
import androidx.media3.common.util.UnstableApi
import androidx.media3.exoplayer.audio.TeeAudioProcessor
import java.nio.ByteBuffer
import java.nio.ByteOrder
import kotlin.math.PI
import kotlin.math.cos
import kotlin.math.ln
import kotlin.math.max
import kotlin.math.pow
import kotlin.math.sin
import kotlin.math.sqrt

/** How many bars the visuals get. */
const val BANDS = 48
private const val FFT_SIZE = 1024
private const val LOWEST_HZ = 40f

/**
 * Turns audio into [BANDS] bars, each 0 to 1, for the visuals. Allocates only
 * up front: it runs on the player's thread for every buffer while switched on.
 */
class SpectrumAnalyzer {
    private val window = FloatArray(FFT_SIZE) { (0.5 - 0.5 * cos(2 * PI * it / (FFT_SIZE - 1))).toFloat() }
    private val cosTable = FloatArray(FFT_SIZE / 2) { cos(2 * PI * it / FFT_SIZE).toFloat() }
    private val sinTable = FloatArray(FFT_SIZE / 2) { sin(2 * PI * it / FFT_SIZE).toFloat() }
    private val real = FloatArray(FFT_SIZE)
    private val imag = FloatArray(FFT_SIZE)
    private val samples = FloatArray(FFT_SIZE)
    private var filled = 0

    /** Written whole, then swapped, so readers never see half of an update. */
    private var spare = FloatArray(BANDS)

    @Volatile
    var bands = FloatArray(BANDS)
        private set

    fun clear() {
        filled = 0
        bands = FloatArray(BANDS)
    }

    /** One mono sample, from -1 to 1. Analyzes when a full window arrives. */
    fun add(sample: Float, sampleRate: Int) {
        samples[filled++] = sample
        if (filled == FFT_SIZE) {
            filled = 0
            analyze(sampleRate)
        }
    }

    private fun analyze(sampleRate: Int) {
        for (i in 0 until FFT_SIZE) {
            real[i] = samples[i] * window[i]
            imag[i] = 0f
        }
        fft()
        val top = sampleRate / 2f
        val out = spare
        for (band in 0 until BANDS) {
            // Log spaced, the way pitch is heard.
            val from = LOWEST_HZ * (top / LOWEST_HZ).pow(band / BANDS.toFloat())
            val to = LOWEST_HZ * (top / LOWEST_HZ).pow((band + 1) / BANDS.toFloat())
            val first = max(1, (from / top * (FFT_SIZE / 2)).toInt())
            val last = max(first, (to / top * (FFT_SIZE / 2)).toInt()).coerceAtMost(FFT_SIZE / 2 - 1)
            var peak = 0f
            for (bin in first..last) {
                val power = real[bin] * real[bin] + imag[bin] * imag[bin]
                if (power > peak) peak = power
            }
            // The window halves the level; this makes a full-scale tone read about 1.
            val magnitude = sqrt(peak) / (FFT_SIZE / 4f)
            val decibels = 20f * ln(magnitude.coerceAtLeast(1e-5f)) / LN_10
            out[band] = ((decibels + FLOOR_DB) / FLOOR_DB).coerceIn(0f, 1f)
        }
        spare = bands
        bands = out
    }

    /** In place, radix 2. */
    private fun fft() {
        var j = 0
        for (i in 1 until FFT_SIZE) {
            var bit = FFT_SIZE shr 1
            while (j and bit != 0) {
                j = j xor bit
                bit = bit shr 1
            }
            j = j xor bit
            if (i < j) {
                val r = real[i]
                real[i] = real[j]
                real[j] = r
                val m = imag[i]
                imag[i] = imag[j]
                imag[j] = m
            }
        }
        var size = 2
        while (size <= FFT_SIZE) {
            val half = size / 2
            val step = FFT_SIZE / size
            var start = 0
            while (start < FFT_SIZE) {
                for (k in 0 until half) {
                    val wr = cosTable[k * step]
                    val wi = -sinTable[k * step]
                    val a = start + k
                    val b = a + half
                    val tr = wr * real[b] - wi * imag[b]
                    val ti = wr * imag[b] + wi * real[b]
                    real[b] = real[a] - tr
                    imag[b] = imag[a] - ti
                    real[a] += tr
                    imag[a] += ti
                }
                start += size
            }
            size *= 2
        }
    }

    private companion object {
        const val FLOOR_DB = 70f
        val LN_10 = ln(10f)
    }
}

/**
 * The tap between the decoder and the speaker. Being inside the player, it
 * needs no microphone permission, unlike Android's own Visualizer. The audio
 * passes through untouched.
 */
@UnstableApi
object Spectrum : TeeAudioProcessor.AudioBufferSink {
    val analyzer = SpectrumAnalyzer()

    /** Only does the work while the visuals are on screen. */
    @Volatile
    var listening = false

    private var sampleRate = 44100
    private var channels = 2
    private var encoding = C.ENCODING_PCM_16BIT

    override fun flush(sampleRateHz: Int, channelCount: Int, encoding: Int) {
        sampleRate = sampleRateHz
        channels = channelCount
        this.encoding = encoding
    }

    override fun handleBuffer(buffer: ByteBuffer) {
        if (!listening || channels <= 0) return
        val data = buffer.asReadOnlyBuffer().order(ByteOrder.nativeOrder())
        when (encoding) {
            C.ENCODING_PCM_16BIT -> while (data.remaining() >= channels * 2) {
                var sum = 0f
                repeat(channels) { sum += data.short / 32768f }
                analyzer.add(sum / channels, sampleRate)
            }
            C.ENCODING_PCM_FLOAT -> while (data.remaining() >= channels * 4) {
                var sum = 0f
                repeat(channels) { sum += data.float }
                analyzer.add(sum / channels, sampleRate)
            }
        }
    }
}
