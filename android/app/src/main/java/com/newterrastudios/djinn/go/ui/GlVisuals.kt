package com.newterrastudios.djinn.go.ui

import android.content.Context
import android.opengl.GLES20
import android.opengl.GLSurfaceView
import android.util.Log
import androidx.media3.common.util.UnstableApi
import com.newterrastudios.djinn.go.player.BANDS
import com.newterrastudios.djinn.go.player.SoundReader
import com.newterrastudios.djinn.go.player.Spectrum
import com.newterrastudios.djinn.go.player.WAVE_POINTS
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.nio.FloatBuffer
import javax.microedition.khronos.egl.EGLConfig
import javax.microedition.khronos.opengles.GL10
import kotlin.math.min

private const val TAG = "DjinnVisuals"

private const val VERTEX = """
attribute vec2 aPos;
varying vec2 vUv;
void main() {
    vUv = aPos * 0.5 + 0.5;
    gl_Position = vec4(aPos, 0.0, 1.0);
}
"""

/** Shared by every picture: the sound as uniforms, and a few tools. */
private const val HEADER = """
#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif
varying vec2 vUv;
uniform float uTime;
uniform vec2 uRes;
uniform float uBass;
uniform float uMid;
uniform float uTreble;
uniform float uBeat;
uniform sampler2D uBands;
uniform sampler2D uWave;
uniform sampler2D uPrev;
const float TAU = 6.2831853;

vec3 palette(float t) {
    return 0.5 + 0.5 * cos(TAU * (t + vec3(0.72, 0.52, 0.20)));
}
float band(float x) { return texture2D(uBands, vec2(clamp(x, 0.0, 1.0), 0.5)).r; }
float wave(float x) { return texture2D(uWave, vec2(clamp(x, 0.0, 1.0), 0.5)).r * 2.0 - 1.0; }
float hash(vec2 p) {
    p = fract(p * vec2(123.34, 456.21));
    p += dot(p, p + 45.32);
    return fract(p.x * p.y);
}
float noise(vec2 p) {
    vec2 i = floor(p);
    vec2 f = fract(p);
    f = f * f * (3.0 - 2.0 * f);
    return mix(mix(hash(i), hash(i + vec2(1.0, 0.0)), f.x),
               mix(hash(i + vec2(0.0, 1.0)), hash(i + vec2(1.0, 1.0)), f.x), f.y);
}
vec2 centered() {
    vec2 p = vUv * 2.0 - 1.0;
    p.x *= uRes.x / uRes.y;
    return p;
}
"""

/**
 * The last frame, turned and pulled in a little, with the sound drawn on top.
 * The trails and swirls come from feeding each frame into the next, the idea
 * MilkDrop made famous.
 */
private const val WARP = """
void main() {
    vec2 p = vUv - 0.5;
    float aspect = uRes.x / uRes.y;
    p.x *= aspect;
    float r = length(p);
    float a = atan(p.y, p.x);
    float spin = 0.012 * sin(uTime * 0.21) + 0.018 * uMid * sin(uTime * 0.53 + r * 5.0);
    a += spin;
    float zoom = 0.985 - 0.035 * uBass - 0.02 * uBeat + 0.006 * sin(a * 5.0 + uTime * 0.7) * uTreble;
    r *= zoom;
    vec2 q = vec2(cos(a), sin(a)) * r;
    q += 0.004 * vec2(sin(uTime * 0.31 + q.y * 9.0), cos(uTime * 0.37 + q.x * 9.0));
    q.x /= aspect;
    vec3 c = texture2D(uPrev, q + 0.5).rgb * (0.955 + 0.03 * uBass);

    vec2 s = centered();
    float angle = atan(s.y, s.x) / TAU + 0.5;
    float radius = length(s);
    float ring = 0.34 + 0.12 * uBass + 0.16 * wave(abs(angle - 0.5) * 2.0);
    float line = smoothstep(0.022, 0.0, abs(radius - ring));
    c += line * palette(uTime * 0.05 + angle) * (0.55 + uBeat);
    float flash = smoothstep(0.4, 0.0, radius) * uBeat * 0.16;
    c += flash * palette(uTime * 0.09 + 0.5);
    gl_FragColor = vec4(min(c, vec3(1.0)), 1.0);
}
"""

private const val TUNNEL = """
void main() {
    vec2 p = centered();
    float r = length(p) + 0.001;
    float a = atan(p.y, p.x) / TAU;
    float depth = 0.45 / r + uTime * (0.35 + 0.5 * uBass);
    float twist = a * 6.0 + depth * 0.5 + uMid * 0.8;
    float rings = 0.5 + 0.5 * sin(depth * 8.0 + uBass * 3.0);
    float slats = 0.5 + 0.5 * sin(twist * TAU * 1.0);
    // Angles jump where they wrap, so color and level use forms that meet there.
    float level = band(abs(a) * 2.0);
    vec3 c = palette(depth * 0.09 + 0.15 * sin(a * TAU)) * (rings * 0.55 + slats * 0.35 + level * 0.8);
    c *= smoothstep(0.0, 0.45, r);
    c += palette(uTime * 0.11) * smoothstep(0.32, 0.0, r) * (0.25 + uBeat * 0.8);
    gl_FragColor = vec4(c, 1.0);
}
"""

private const val PLASMA = """
void main() {
    vec2 p = centered() * (1.6 + uBass * 0.7);
    float t = uTime * 0.45;
    float v = sin(p.x * 2.1 + t) + sin(p.y * 2.7 - t * 1.3)
        + sin((p.x + p.y) * 1.7 + t * 0.7) + sin(length(p) * 3.3 - t * 1.9 + uMid * 3.0);
    v += 0.8 * sin(p.x * 5.0 * uTreble + p.y * 3.0 + t);
    vec3 c = palette(v * 0.17 + uTime * 0.03);
    c *= 0.55 + 0.45 * sin(v * 2.2 + uBeat * 2.0);
    c += 0.28 * uBeat * palette(v * 0.3);
    gl_FragColor = vec4(c, 1.0);
}
"""

private const val KALEIDO = """
void main() {
    vec2 p = centered();
    float r = length(p);
    float a = atan(p.y, p.x);
    float slices = 8.0;
    float side = TAU / slices;
    a = mod(a + uTime * 0.08, side);
    a = abs(a - side * 0.5);
    vec2 q = r * vec2(cos(a), sin(a));
    q *= 1.4 + 0.6 * uBass;
    q += vec2(uTime * 0.11, sin(uTime * 0.13));
    float n = noise(q * 3.0) * 0.6 + noise(q * 7.0 + uTime * 0.2) * 0.3 + noise(q * 15.0) * 0.1;
    float level = band(r * 0.7);
    float edge = smoothstep(0.02, 0.0, abs(fract(n * 4.0 + uMid) - 0.5) - 0.2);
    vec3 c = palette(n + r * 0.3 + uTime * 0.04) * (0.25 + edge * 0.7 + level * 0.75);
    c *= smoothstep(1.7, 0.2, r);
    c += uBeat * 0.2 * palette(r + uTime * 0.1);
    gl_FragColor = vec4(c, 1.0);
}
"""

private const val NEBULA = """
float fbm(vec2 p) {
    float v = 0.0;
    float a = 0.5;
    for (int i = 0; i < 4; i++) {
        v += a * noise(p);
        p = p * 2.03 + vec2(1.7, 9.2);
        a *= 0.5;
    }
    return v;
}
void main() {
    vec2 p = centered() * 1.3;
    float t = uTime * 0.07;
    vec2 w = vec2(fbm(p + vec2(t, -t)), fbm(p + vec2(5.2 - t, 1.3 + t)));
    float n = fbm(p + 2.2 * w + uBass * 0.5);
    vec3 c = palette(n * 0.9 + w.x * 0.4 + uTime * 0.015) * smoothstep(0.15, 0.65, n);
    c *= 0.9 + uMid * 1.1;
    float glow = smoothstep(1.1, 0.0, length(p));
    c += glow * glow * 0.3 * uBass * palette(uTime * 0.04);
    c += uBeat * 0.14 * palette(n);
    gl_FragColor = vec4(c, 1.0);
}
"""

private const val STARS = """
vec3 layer(vec2 p, float z) {
    vec2 cell = floor(p);
    vec2 f = fract(p) - 0.5;
    float id = hash(cell + z);
    vec2 at = vec2(hash(cell + 1.3), hash(cell + 7.7)) - 0.5;
    float d = length(f - at * 0.7);
    float size = 0.02 + 0.05 * id * (0.5 + uTreble);
    return palette(id + uTime * 0.02) * smoothstep(size, 0.0, d);
}
void main() {
    vec2 p = centered();
    vec3 c = vec3(0.0);
    for (int i = 0; i < 5; i++) {
        float z = fract(float(i) / 5.0 + uTime * (0.05 + 0.03 * uBass));
        float scale = mix(9.0, 0.6, z);
        float fade = smoothstep(0.0, 0.35, z) * smoothstep(1.0, 0.8, z);
        c += layer(p * scale + float(i) * 3.7, float(i)) * fade * 1.6;
    }
    float r = length(p);
    c += palette(uTime * 0.03 + r) * smoothstep(0.9, 0.0, r) * (0.07 + 0.3 * uBass);
    c += uBeat * 0.12 * palette(uTime * 0.08);
    gl_FragColor = vec4(c, 1.0);
}
"""

/** Shows the feedback picture on screen. */
private const val SHOW = """
precision mediump float;
varying vec2 vUv;
uniform sampler2D uPrev;
void main() { gl_FragColor = texture2D(uPrev, vUv); }
"""

/** The source of a picture's fragment shader, for the styles drawn on the graphics chip. */
fun fragmentFor(style: VisualStyle): String? = when (style) {
    VisualStyle.Warp -> HEADER + WARP
    VisualStyle.Tunnel -> HEADER + TUNNEL
    VisualStyle.Plasma -> HEADER + PLASMA
    VisualStyle.Kaleidoscope -> HEADER + KALEIDO
    VisualStyle.Nebula -> HEADER + NEBULA
    VisualStyle.Stars -> HEADER + STARS
    else -> null
}

private class Program(val id: Int) {
    val pos = GLES20.glGetAttribLocation(id, "aPos")
    val time = GLES20.glGetUniformLocation(id, "uTime")
    val res = GLES20.glGetUniformLocation(id, "uRes")
    val bass = GLES20.glGetUniformLocation(id, "uBass")
    val mid = GLES20.glGetUniformLocation(id, "uMid")
    val treble = GLES20.glGetUniformLocation(id, "uTreble")
    val beat = GLES20.glGetUniformLocation(id, "uBeat")
    val bands = GLES20.glGetUniformLocation(id, "uBands")
    val wave = GLES20.glGetUniformLocation(id, "uWave")
    val prev = GLES20.glGetUniformLocation(id, "uPrev")
}

/** Draws one style. Everything runs on the view's own drawing thread. */
@UnstableApi
class VisualsRenderer(initial: VisualStyle) : GLSurfaceView.Renderer {
    @Volatile
    var style = initial

    /** Why a picture couldn't be built, for the device tests. */
    val failures = mutableListOf<String>()

    private val sound = SoundReader(Spectrum.analyzer)
    private val programs = HashMap<VisualStyle, Program?>()
    private var show: Program? = null
    private var width = 1
    private var height = 1
    private var bandsTexture = 0
    private var waveTexture = 0
    private val bandBytes: ByteBuffer = ByteBuffer.allocateDirect(BANDS).order(ByteOrder.nativeOrder())
    private val waveBytes: ByteBuffer = ByteBuffer.allocateDirect(WAVE_POINTS).order(ByteOrder.nativeOrder())
    private val quad: FloatBuffer = ByteBuffer.allocateDirect(8 * 4).order(ByteOrder.nativeOrder()).asFloatBuffer()
        .apply {
            put(floatArrayOf(-1f, -1f, 1f, -1f, -1f, 1f, 1f, 1f))
            position(0)
        }

    // The two pictures feedback styles take turns drawing into and reading from.
    private val frameBuffers = IntArray(2)
    private val frameTextures = IntArray(2)
    private var front = 0
    private var startNanos = 0L

    override fun onSurfaceCreated(gl: GL10?, config: EGLConfig?) {
        programs.clear()
        show = null
        failures.clear()
        startNanos = System.nanoTime()
        val textures = IntArray(2)
        GLES20.glGenTextures(2, textures, 0)
        bandsTexture = textures[0]
        waveTexture = textures[1]
        for (texture in textures) {
            GLES20.glBindTexture(GLES20.GL_TEXTURE_2D, texture)
            setTextureRules()
        }
        GLES20.glDisable(GLES20.GL_DEPTH_TEST)
        GLES20.glDisable(GLES20.GL_BLEND)
    }

    private fun setTextureRules() {
        GLES20.glTexParameteri(GLES20.GL_TEXTURE_2D, GLES20.GL_TEXTURE_MIN_FILTER, GLES20.GL_LINEAR)
        GLES20.glTexParameteri(GLES20.GL_TEXTURE_2D, GLES20.GL_TEXTURE_MAG_FILTER, GLES20.GL_LINEAR)
        GLES20.glTexParameteri(GLES20.GL_TEXTURE_2D, GLES20.GL_TEXTURE_WRAP_S, GLES20.GL_CLAMP_TO_EDGE)
        GLES20.glTexParameteri(GLES20.GL_TEXTURE_2D, GLES20.GL_TEXTURE_WRAP_T, GLES20.GL_CLAMP_TO_EDGE)
    }

    override fun onSurfaceChanged(gl: GL10?, w: Int, h: Int) {
        width = w.coerceAtLeast(1)
        height = h.coerceAtLeast(1)
        buildFeedback()
    }

    /** Feedback is soft by nature, so it can run smaller than the screen and stay fast. */
    private fun buildFeedback() {
        if (frameBuffers[0] != 0) {
            GLES20.glDeleteFramebuffers(2, frameBuffers, 0)
            GLES20.glDeleteTextures(2, frameTextures, 0)
        }
        val scale = min(1f, FEEDBACK_MAX / maxOf(width, height).toFloat())
        val w = (width * scale).toInt().coerceAtLeast(1)
        val h = (height * scale).toInt().coerceAtLeast(1)
        GLES20.glGenFramebuffers(2, frameBuffers, 0)
        GLES20.glGenTextures(2, frameTextures, 0)
        for (i in 0..1) {
            GLES20.glBindTexture(GLES20.GL_TEXTURE_2D, frameTextures[i])
            setTextureRules()
            GLES20.glTexImage2D(GLES20.GL_TEXTURE_2D, 0, GLES20.GL_RGBA, w, h, 0, GLES20.GL_RGBA, GLES20.GL_UNSIGNED_BYTE, null)
            GLES20.glBindFramebuffer(GLES20.GL_FRAMEBUFFER, frameBuffers[i])
            GLES20.glFramebufferTexture2D(GLES20.GL_FRAMEBUFFER, GLES20.GL_COLOR_ATTACHMENT0, GLES20.GL_TEXTURE_2D, frameTextures[i], 0)
            GLES20.glClearColor(0f, 0f, 0f, 1f)
            GLES20.glClear(GLES20.GL_COLOR_BUFFER_BIT)
        }
        GLES20.glBindFramebuffer(GLES20.GL_FRAMEBUFFER, 0)
        feedbackWidth = w
        feedbackHeight = h
    }

    private var feedbackWidth = 1
    private var feedbackHeight = 1

    private fun compile(type: Int, source: String, name: String): Int {
        val shader = GLES20.glCreateShader(type)
        GLES20.glShaderSource(shader, source)
        GLES20.glCompileShader(shader)
        val ok = IntArray(1)
        GLES20.glGetShaderiv(shader, GLES20.GL_COMPILE_STATUS, ok, 0)
        if (ok[0] == 0) {
            val why = GLES20.glGetShaderInfoLog(shader)
            failures += "$name: $why"
            Log.w(TAG, "$name wouldn't compile: $why")
            GLES20.glDeleteShader(shader)
            return 0
        }
        return shader
    }

    private fun link(fragmentSource: String, name: String): Program? {
        val vertex = compile(GLES20.GL_VERTEX_SHADER, VERTEX, name)
        val fragment = compile(GLES20.GL_FRAGMENT_SHADER, fragmentSource, name)
        if (vertex == 0 || fragment == 0) return null
        val program = GLES20.glCreateProgram()
        GLES20.glAttachShader(program, vertex)
        GLES20.glAttachShader(program, fragment)
        GLES20.glLinkProgram(program)
        val ok = IntArray(1)
        GLES20.glGetProgramiv(program, GLES20.GL_LINK_STATUS, ok, 0)
        if (ok[0] == 0) {
            failures += "$name: ${GLES20.glGetProgramInfoLog(program)}"
            return null
        }
        return Program(program)
    }

    private fun program(style: VisualStyle): Program? = programs.getOrPut(style) {
        fragmentFor(style)?.let { link(it, style.name) }
    }

    private fun upload() {
        bandBytes.position(0)
        for (i in 0 until BANDS) bandBytes.put((sound.bands[i] * 255f).toInt().coerceIn(0, 255).toByte())
        bandBytes.position(0)
        GLES20.glActiveTexture(GLES20.GL_TEXTURE1)
        GLES20.glBindTexture(GLES20.GL_TEXTURE_2D, bandsTexture)
        GLES20.glPixelStorei(GLES20.GL_UNPACK_ALIGNMENT, 1)
        GLES20.glTexImage2D(GLES20.GL_TEXTURE_2D, 0, GLES20.GL_LUMINANCE, BANDS, 1, 0, GLES20.GL_LUMINANCE, GLES20.GL_UNSIGNED_BYTE, bandBytes)
        waveBytes.position(0)
        for (i in 0 until WAVE_POINTS) waveBytes.put(((sound.wave[i] * 0.5f + 0.5f) * 255f).toInt().coerceIn(0, 255).toByte())
        waveBytes.position(0)
        GLES20.glActiveTexture(GLES20.GL_TEXTURE2)
        GLES20.glBindTexture(GLES20.GL_TEXTURE_2D, waveTexture)
        GLES20.glTexImage2D(GLES20.GL_TEXTURE_2D, 0, GLES20.GL_LUMINANCE, WAVE_POINTS, 1, 0, GLES20.GL_LUMINANCE, GLES20.GL_UNSIGNED_BYTE, waveBytes)
    }

    private fun drawQuad(program: Program, w: Int, h: Int, seconds: Float, previous: Int) {
        GLES20.glUseProgram(program.id)
        GLES20.glViewport(0, 0, w, h)
        GLES20.glUniform1f(program.time, seconds)
        GLES20.glUniform2f(program.res, width.toFloat(), height.toFloat())
        GLES20.glUniform1f(program.bass, sound.bass)
        GLES20.glUniform1f(program.mid, sound.mid)
        GLES20.glUniform1f(program.treble, sound.treble)
        GLES20.glUniform1f(program.beat, sound.beat)
        GLES20.glActiveTexture(GLES20.GL_TEXTURE0)
        GLES20.glBindTexture(GLES20.GL_TEXTURE_2D, previous)
        GLES20.glUniform1i(program.prev, 0)
        GLES20.glUniform1i(program.bands, 1)
        GLES20.glUniform1i(program.wave, 2)
        quad.position(0)
        GLES20.glEnableVertexAttribArray(program.pos)
        GLES20.glVertexAttribPointer(program.pos, 2, GLES20.GL_FLOAT, false, 0, quad)
        GLES20.glDrawArrays(GLES20.GL_TRIANGLE_STRIP, 0, 4)
    }

    override fun onDrawFrame(gl: GL10?) {
        val now = System.nanoTime()
        sound.read(now, Spectrum.leadMs, Spectrum.intensity)
        upload()
        // Kept small so precision holds on phones whose shaders have little of it.
        val seconds = (((now - startNanos) / 1_000_000L) % WRAP_MS) / 1000f
        val current = style
        val program = program(current)
        if (program == null) {
            GLES20.glClearColor(0f, 0f, 0f, 1f)
            GLES20.glBindFramebuffer(GLES20.GL_FRAMEBUFFER, 0)
            GLES20.glClear(GLES20.GL_COLOR_BUFFER_BIT)
            return
        }
        if (current == VisualStyle.Warp) {
            val back = 1 - front
            GLES20.glBindFramebuffer(GLES20.GL_FRAMEBUFFER, frameBuffers[back])
            drawQuad(program, feedbackWidth, feedbackHeight, seconds, frameTextures[front])
            front = back
            GLES20.glBindFramebuffer(GLES20.GL_FRAMEBUFFER, 0)
            val shower = show ?: link(SHOW, "show").also { show = it } ?: return
            drawQuad(shower, width, height, seconds, frameTextures[front])
        } else if (current in HEAVY) {
            // Soft pictures anyway, so they're drawn smaller and stretched to stay smooth.
            GLES20.glBindFramebuffer(GLES20.GL_FRAMEBUFFER, frameBuffers[front])
            drawQuad(program, feedbackWidth, feedbackHeight, seconds, frameTextures[1 - front])
            GLES20.glBindFramebuffer(GLES20.GL_FRAMEBUFFER, 0)
            val shower = show ?: link(SHOW, "show").also { show = it } ?: return
            drawQuad(shower, width, height, seconds, frameTextures[front])
        } else {
            GLES20.glBindFramebuffer(GLES20.GL_FRAMEBUFFER, 0)
            drawQuad(program, width, height, seconds, frameTextures[front])
        }
    }

    private companion object {
        const val FEEDBACK_MAX = 720f
        const val WRAP_MS = 3_600_000L
        val HEAVY = setOf(VisualStyle.Kaleidoscope, VisualStyle.Nebula, VisualStyle.Stars)
    }
}

/** The surface the graphics chip draws on, stopping when nothing shows it. */
@UnstableApi
class VisualsView(context: Context, style: VisualStyle) : GLSurfaceView(context) {
    val visuals = VisualsRenderer(style)

    init {
        setEGLContextClientVersion(2)
        preserveEGLContextOnPause = true
        setRenderer(visuals)
        renderMode = RENDERMODE_CONTINUOUSLY
    }
}
