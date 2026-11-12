package com.newterrastudios.djinn.go.player

import androidx.media3.common.C
import androidx.media3.common.audio.AudioProcessor
import androidx.media3.common.audio.BaseAudioProcessor
import androidx.media3.common.util.UnstableApi
import java.nio.ByteBuffer
import java.nio.ByteOrder
import kotlin.math.PI
import kotlin.math.abs
import kotlin.math.cos
import kotlin.math.pow
import kotlin.math.sign
import kotlin.math.sin
import kotlin.math.tanh

/**
 * Your own equalizer: the same twelve bands, presets and limits as the
 * party's (djinn-engine's equalizer.rs) and web GO's, kept on this phone.
 */
data class EqSettings(
    val enabled: Boolean = false,
    val bandsDb: List<Float> = List(EQ_BAND_HZ.size) { 0f },
    val outputGainDb: Float = 0f,
)

val EQ_BAND_HZ = intArrayOf(32, 64, 125, 250, 500, 1000, 2000, 4000, 6000, 8000, 12000, 16000)
const val EQ_MAX_BAND_DB = 12f
const val EQ_MIN_OUTPUT_DB = -12f
const val EQ_MAX_OUTPUT_DB = 6f

/** Presets by translation key, the same as the web's. */
val EQ_PRESETS: List<Pair<String, List<Float>>> = listOf(
    "eq.flat" to listOf(0f, 0f, 0f, 0f, 0f, 0f, 0f, 0f, 0f, 0f, 0f, 0f),
    "eq.bass" to listOf(4f, 5f, 5f, 3f, 1f, 0f, 0f, 0f, 0f, 0f, 0f, 0f),
    "eq.vocals" to listOf(0f, -1f, -1f, 0f, 1f, 2f, 3f, 2f, 1f, 0f, 0f, 0f),
    "eq.rock" to listOf(3f, 4f, 3f, 1f, -1f, 0f, 2f, 3f, 2f, 1f, 1f, 0f),
    "eq.electronic" to listOf(4f, 5f, 3f, 1f, 0f, -1f, 0f, 2f, 3f, 2f, 1f, 0f),
    "eq.bright" to listOf(0f, 0f, 0f, 0f, 0f, 0f, 1f, 2f, 2f, 3f, 3f, 2f),
)

/** What the player applies; the settings screen sets it. */
object GoEqualizer {
    @Volatile
    var settings: EqSettings = EqSettings()
}

private const val Q = 1.4
/** Above this the output is eased toward full scale rather than clipped. */
private const val SOFT_FROM = 0.9f

/**
 * Twelve peaking filters (W3C Audio EQ Cookbook) on the decoded sound,
 * before the visuals read it. Always in the chain so turning it on or off
 * needs no restart; while off it only copies.
 */
@UnstableApi
class EqualizerProcessor : BaseAudioProcessor() {
    private var applied: EqSettings? = null
    /** Per band: b0, b1, b2, a1, a2 (a0 = 1); empty while flat or off. */
    private var coefficients: Array<DoubleArray> = emptyArray()
    /** Per band and channel: x1, x2, y1, y2. */
    private var memory: Array<Array<DoubleArray>> = emptyArray()
    private var gain = 1f

    override fun onConfigure(inputAudioFormat: AudioProcessor.AudioFormat): AudioProcessor.AudioFormat {
        if (inputAudioFormat.encoding != C.ENCODING_PCM_16BIT && inputAudioFormat.encoding != C.ENCODING_PCM_FLOAT) {
            throw AudioProcessor.UnhandledAudioFormatException(inputAudioFormat)
        }
        applied = null
        return inputAudioFormat
    }

    private fun refresh() {
        val settings = GoEqualizer.settings
        if (settings === applied) return
        applied = settings
        val rate = inputAudioFormat.sampleRate.toDouble().coerceAtLeast(1.0)
        val channels = inputAudioFormat.channelCount.coerceAtLeast(1)
        val active = if (settings.enabled) {
            EQ_BAND_HZ.indices.filter { abs(settings.bandsDb.getOrElse(it) { 0f }) > 0.01f }
        } else {
            emptyList()
        }
        coefficients = active.map { i ->
            peak(rate, EQ_BAND_HZ[i].toDouble(), settings.bandsDb[i].coerceIn(-EQ_MAX_BAND_DB, EQ_MAX_BAND_DB).toDouble())
        }.toTypedArray()
        // Filters that stay keep their memory, so a change doesn't click much.
        memory = Array(coefficients.size) { band -> memory.getOrNull(band)?.takeIf { it.size == channels } ?: Array(channels) { DoubleArray(4) } }
        gain = if (settings.enabled) {
            10f.pow(settings.outputGainDb.coerceIn(EQ_MIN_OUTPUT_DB, EQ_MAX_OUTPUT_DB) / 20f)
        } else {
            1f
        }
    }

    private fun peak(rate: Double, frequency: Double, gainDb: Double): DoubleArray {
        val omega = 2.0 * PI * frequency.coerceAtMost(rate * 0.45) / rate
        val alpha = sin(omega) / (2.0 * Q)
        val cosine = cos(omega)
        val a = 10.0.pow(gainDb / 40.0)
        val a0 = 1.0 + alpha / a
        return doubleArrayOf(
            (1.0 + alpha * a) / a0,
            -2.0 * cosine / a0,
            (1.0 - alpha * a) / a0,
            -2.0 * cosine / a0,
            (1.0 - alpha / a) / a0,
        )
    }

    private fun filter(sample: Float, channel: Int): Float {
        var x = sample.toDouble()
        for (band in coefficients.indices) {
            val c = coefficients[band]
            val m = memory[band][channel]
            val y = c[0] * x + c[1] * m[0] + c[2] * m[1] - c[3] * m[2] - c[4] * m[3]
            m[1] = m[0]
            m[0] = x
            m[3] = m[2]
            m[2] = y
            x = y
        }
        val out = x.toFloat() * gain
        return if (abs(out) <= SOFT_FROM) out else sign(out) * (SOFT_FROM + (1f - SOFT_FROM) * tanh((abs(out) - SOFT_FROM) / (1f - SOFT_FROM)))
    }

    override fun queueInput(inputBuffer: ByteBuffer) {
        val size = inputBuffer.remaining()
        if (size == 0) return
        refresh()
        val output = replaceOutputBuffer(size)
        if (coefficients.isEmpty() && gain == 1f) {
            output.put(inputBuffer)
            output.flip()
            return
        }
        val input = inputBuffer.order(ByteOrder.nativeOrder())
        val channels = inputAudioFormat.channelCount.coerceAtLeast(1)
        var channel = 0
        if (inputAudioFormat.encoding == C.ENCODING_PCM_FLOAT) {
            while (input.remaining() >= 4) {
                output.putFloat(filter(input.getFloat(), channel))
                channel = (channel + 1) % channels
            }
        } else {
            while (input.remaining() >= 2) {
                val y = filter(input.getShort() / 32768f, channel)
                output.putShort((y * 32767f).toInt().coerceIn(-32768, 32767).toShort())
                channel = (channel + 1) % channels
            }
        }
        output.flip()
    }

    override fun onFlush() {
        memory.forEach { band -> band.forEach { it.fill(0.0) } }
    }

    override fun onReset() {
        applied = null
        coefficients = emptyArray()
        memory = emptyArray()
        gain = 1f
    }
}
