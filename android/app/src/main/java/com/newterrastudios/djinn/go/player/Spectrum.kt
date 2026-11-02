package com.newterrastudios.djinn.go.player

import androidx.media3.common.C
import androidx.media3.common.util.UnstableApi
import androidx.media3.exoplayer.audio.TeeAudioProcessor
import java.nio.ByteBuffer
import java.nio.ByteOrder
import kotlin.math.PI
import kotlin.math.abs
import kotlin.math.cos
import kotlin.math.ln
import kotlin.math.max
import kotlin.math.pow
import kotlin.math.sin
import kotlin.math.sqrt

/** How many bars the visuals get. */
const val BANDS = 48

/** Points in the waveform the visuals draw. */
const val WAVE_POINTS = 256
private const val FFT_SIZE = 1024
private const val LOWEST_HZ = 40f

/** Frames kept for the visuals to catch up with the speaker: over a second. */
private const val FRAMES = 128

/** One moment of the sound. */
class SoundFrame {
    /** When, on the stream's own clock, in microseconds. */
    @Volatile
    var atUs = -1L
    val bands = FloatArray(BANDS)
    val wave = FloatArray(WAVE_POINTS)
    var bass = 0f
    var mid = 0f
    var treble = 0f
    var beat = false
}

/**
 * Turns audio into what the visuals draw: bars, a waveform, how much bass, mid
 * and treble, and beats. Allocates only up front: it runs on the player's
 * thread for every buffer while switched on.
 */
class SpectrumAnalyzer {
    private val window = FloatArray(FFT_SIZE) { (0.5 - 0.5 * cos(2 * PI * it / (FFT_SIZE - 1))).toFloat() }
    private val cosTable = FloatArray(FFT_SIZE / 2) { cos(2 * PI * it / FFT_SIZE).toFloat() }
    private val sinTable = FloatArray(FFT_SIZE / 2) { sin(2 * PI * it / FFT_SIZE).toFloat() }
    private val real = FloatArray(FFT_SIZE)
    private val imag = FloatArray(FFT_SIZE)
    private val samples = FloatArray(FFT_SIZE)
    private var filled = 0
    private val frames = Array(FRAMES) { SoundFrame() }

    /** Frames written so far; the newest is this minus one. */
    @Volatile
    private var written = 0L
    private var samplesSeen = 0L
    private var bassAverage = 0f
    private var lastBeatUs = 0L

    /** The newest bars. */
    val bands: FloatArray
        get() = latest()?.bands ?: EMPTY

    fun latest(): SoundFrame? {
        val count = written
        return if (count == 0L) null else frames[((count - 1) % FRAMES).toInt()]
    }

    /** The newest frame at or before a moment, or null if there's none kept. */
    fun at(us: Long): SoundFrame? {
        val count = written
        var back = 1L
        while (back <= count && back <= FRAMES - 2) {
            val frame = frames[((count - back) % FRAMES).toInt()]
            if (frame.atUs in 0..us) return frame
            back++
        }
        return null
    }

    /** A new stream or a seek: nothing before this counts. */
    fun clear() {
        filled = 0
        for (frame in frames) frame.atUs = -1
        written = 0
        samplesSeen = 0
        bassAverage = 0f
        lastBeatUs = 0
    }

    /** One mono sample, from -1 to 1. Analyzes when a full window arrives. */
    fun add(sample: Float, sampleRate: Int) {
        samples[filled++] = sample
        samplesSeen++
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
        val frame = frames[(written % FRAMES).toInt()]
        // Being rewritten: not to be picked while it's half done.
        frame.atUs = -1
        val top = sampleRate / 2f
        var bass = 0f
        var bassCount = 0
        var mid = 0f
        var midCount = 0
        var treble = 0f
        var trebleCount = 0
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
            val level = ((decibels + FLOOR_DB) / FLOOR_DB).coerceIn(0f, 1f)
            frame.bands[band] = level
            when {
                from < BASS_TOP_HZ -> {
                    bass += level
                    bassCount++
                }
                from < MID_TOP_HZ -> {
                    mid += level
                    midCount++
                }
                else -> {
                    treble += level
                    trebleCount++
                }
            }
        }
        frame.bass = if (bassCount > 0) bass / bassCount else 0f
        frame.mid = if (midCount > 0) mid / midCount else 0f
        frame.treble = if (trebleCount > 0) treble / trebleCount else 0f
        val step = FFT_SIZE / WAVE_POINTS
        for (i in 0 until WAVE_POINTS) frame.wave[i] = samples[i * step]

        val now = samplesSeen * 1_000_000L / sampleRate
        frame.beat = frame.bass > bassAverage + BEAT_RISE && frame.bass > BEAT_FLOOR && now - lastBeatUs > BEAT_GAP_US
        if (frame.beat) lastBeatUs = now
        bassAverage += (frame.bass - bassAverage) * BASS_FOLLOW
        frame.atUs = now
        written += 1
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
        const val BASS_TOP_HZ = 250f
        const val MID_TOP_HZ = 2000f
        /** A beat is bass this far over what it has been lately. */
        const val BEAT_RISE = 0.08f
        const val BEAT_FLOOR = 0.35f
        const val BEAT_GAP_US = 180_000L
        const val BASS_FOLLOW = 0.08f
        val LN_10 = ln(10f)
        val EMPTY = FloatArray(BANDS)
    }
}

/**
 * What a visual reads each frame. The player hands audio over in bursts and
 * ahead of the speaker, so this keeps its own steady clock that runs with real
 * time and leans gently toward "newest audio, less the lead". Reacting to the
 * sound as it's heard rather than as it arrives is what makes visuals feel in
 * time. Used from one thread at a time.
 */
class SoundReader(private val analyzer: SpectrumAnalyzer) {
    val bands = FloatArray(BANDS)
    val wave = FloatArray(WAVE_POINTS)
    var bass = 0f
        private set
    var mid = 0f
        private set
    var treble = 0f
        private set

    /** 1 on a beat, falling away to 0. */
    var beat = 0f
        private set

    private var clockUs = Long.MIN_VALUE
    private var lastNanos = 0L
    private var lastFrameAt = -1L
    private var lastNewest = -1L
    private var lastChangeNanos = 0L

    /**
     * Moves on to `nowNanos` and fills the fields. `leadMs` is how far the
     * audio here is ahead of the speaker; `gain` scales how strongly it shows.
     */
    fun read(nowNanos: Long, leadMs: Int, gain: Float = 1f) {
        val elapsedUs = if (lastNanos == 0L) 0L else ((nowNanos - lastNanos) / 1000).coerceIn(0, 200_000)
        lastNanos = nowNanos
        val newest = analyzer.latest()?.atUs ?: -1L
        if (newest != lastNewest) {
            // The stream went back: a seek or a new song.
            if (newest < lastNewest) clockUs = Long.MIN_VALUE
            lastNewest = newest
            lastChangeNanos = nowNanos
        }
        val silent = newest < 0 || nowNanos - lastChangeNanos > QUIET_AFTER_NANOS
        val target = newest - leadMs * 1000L
        if (clockUs == Long.MIN_VALUE || abs(target - clockUs) > SNAP_US) {
            clockUs = target
        } else if (!silent) {
            clockUs += elapsedUs + ((target - clockUs) * LEAN).toLong()
        }
        val frame = if (silent) null else analyzer.at(clockUs)
        val fall = FALL_PER_SECOND * elapsedUs / 1_000_000f
        for (i in 0 until BANDS) {
            val level = ((frame?.bands?.get(i) ?: 0f) * gain).coerceIn(0f, 1f)
            bands[i] = max(level, bands[i] - fall)
        }
        for (i in 0 until WAVE_POINTS) {
            val point = (frame?.wave?.get(i) ?: 0f) * gain
            wave[i] += (point - wave[i]) * WAVE_FOLLOW
        }
        bass = max(((frame?.bass ?: 0f) * gain).coerceIn(0f, 1f), bass - fall)
        mid = max(((frame?.mid ?: 0f) * gain).coerceIn(0f, 1f), mid - fall)
        treble = max(((frame?.treble ?: 0f) * gain).coerceIn(0f, 1f), treble - fall)
        val since = frame?.atUs ?: -1L
        val fresh = since != lastFrameAt
        lastFrameAt = since
        beat = if (fresh && frame?.beat == true) 1f else (beat - BEAT_FALL_PER_SECOND * elapsedUs / 1_000_000f).coerceAtLeast(0f)
    }

    private companion object {
        const val SNAP_US = 1_000_000L
        const val LEAN = 0.03f
        const val QUIET_AFTER_NANOS = 400_000_000L
        const val FALL_PER_SECOND = 1.6f
        const val BEAT_FALL_PER_SECOND = 4f
        const val WAVE_FOLLOW = 0.6f
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

    /** How many visuals are on screen; it only does the work while there's one. */
    @Volatile
    private var watchers = 0

    /** How far audio here is ahead of the speaker, from Settings. */
    @Volatile
    var leadMs = 250

    /** How strongly visuals react, from Settings. */
    @Volatile
    var intensity = 1f

    private var sampleRate = 44100
    private var channels = 2
    private var encoding = C.ENCODING_PCM_16BIT

    @Synchronized
    fun watch() {
        watchers++
    }

    @Synchronized
    fun unwatch() {
        watchers = (watchers - 1).coerceAtLeast(0)
    }

    override fun flush(sampleRateHz: Int, channelCount: Int, encoding: Int) {
        sampleRate = sampleRateHz
        channels = channelCount
        this.encoding = encoding
        analyzer.clear()
    }

    override fun handleBuffer(buffer: ByteBuffer) {
        if (watchers == 0 || channels <= 0) return
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
