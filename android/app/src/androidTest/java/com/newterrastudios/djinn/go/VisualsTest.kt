package com.newterrastudios.djinn.go

import android.opengl.EGL14
import android.opengl.GLES20
import android.util.Log
import androidx.media3.common.util.UnstableApi
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.newterrastudios.djinn.go.player.Spectrum
import com.newterrastudios.djinn.go.ui.VisualStyle
import com.newterrastudios.djinn.go.ui.VisualsRenderer
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import java.nio.ByteBuffer
import kotlin.math.PI
import kotlin.math.sin

/**
 * Builds and draws every shader on this device's real graphics chip, off screen.
 * Graphics chips disagree about shaders, so this is worth running on each kind.
 * It needs no sign-in, shows nothing, and makes no sound.
 */
@UnstableApi
@RunWith(AndroidJUnit4::class)
class VisualsTest {
    private var display = EGL14.EGL_NO_DISPLAY
    private var context = EGL14.EGL_NO_CONTEXT
    private var surface = EGL14.EGL_NO_SURFACE
    /** About a phone's width, so the timings below mean something. */
    private val size = 1080

    @Before
    fun openGraphics() {
        display = EGL14.eglGetDisplay(EGL14.EGL_DEFAULT_DISPLAY)
        val version = IntArray(2)
        assertTrue(EGL14.eglInitialize(display, version, 0, version, 1))
        val configs = arrayOfNulls<android.opengl.EGLConfig>(1)
        val count = IntArray(1)
        EGL14.eglChooseConfig(
            display,
            intArrayOf(
                EGL14.EGL_RENDERABLE_TYPE, EGL14.EGL_OPENGL_ES2_BIT,
                EGL14.EGL_SURFACE_TYPE, EGL14.EGL_PBUFFER_BIT,
                EGL14.EGL_RED_SIZE, 8, EGL14.EGL_GREEN_SIZE, 8, EGL14.EGL_BLUE_SIZE, 8,
                EGL14.EGL_NONE,
            ),
            0, configs, 0, 1, count, 0,
        )
        context = EGL14.eglCreateContext(display, configs[0], EGL14.EGL_NO_CONTEXT, intArrayOf(EGL14.EGL_CONTEXT_CLIENT_VERSION, 2, EGL14.EGL_NONE), 0)
        surface = EGL14.eglCreatePbufferSurface(display, configs[0], intArrayOf(EGL14.EGL_WIDTH, size, EGL14.EGL_HEIGHT, size, EGL14.EGL_NONE), 0)
        assertTrue(EGL14.eglMakeCurrent(display, surface, surface, context))
    }

    @After
    fun closeGraphics() {
        EGL14.eglMakeCurrent(display, EGL14.EGL_NO_SURFACE, EGL14.EGL_NO_SURFACE, EGL14.EGL_NO_CONTEXT)
        EGL14.eglDestroySurface(display, surface)
        EGL14.eglDestroyContext(display, context)
    }

    /** Sound for the renderer to read, so the pictures react as they would. */
    private fun feedSound() {
        val rate = 44100
        Spectrum.analyzer.clear()
        for (i in 0 until rate) {
            val thump = if (i % (rate / 2) < rate / 16) 0.8f else 0.05f
            val sample = thump * sin(2 * PI * 80 * i / rate).toFloat() + 0.2f * sin(2 * PI * 3000 * i / rate).toFloat()
            Spectrum.analyzer.add(sample, rate)
        }
    }

    /** How much of the picture isn't black. */
    private fun litShare(): Float {
        val pixels = ByteBuffer.allocateDirect(size * size * 4)
        GLES20.glReadPixels(0, 0, size, size, GLES20.GL_RGBA, GLES20.GL_UNSIGNED_BYTE, pixels)
        var lit = 0
        for (i in 0 until size * size) {
            val brightest = maxOf(pixels.get(i * 4).toInt() and 255, pixels.get(i * 4 + 1).toInt() and 255, pixels.get(i * 4 + 2).toInt() and 255)
            if (brightest > 8) lit++
        }
        return lit.toFloat() / (size * size)
    }

    @Test
    fun everyShaderBuildsAndDrawsAPicture() {
        val shaderStyles = VisualStyle.entries.filter { it.onGraphicsChip }
        assertEquals(6, shaderStyles.size)
        for (style in shaderStyles) {
            val renderer = VisualsRenderer(style)
            renderer.onSurfaceCreated(null, null)
            renderer.onSurfaceChanged(null, size, size)
            var lit = 0f
            val started = System.nanoTime()
            repeat(FRAMES) {
                feedSoundOnce(it)
                renderer.onDrawFrame(null)
                GLES20.glFinish()
            }
            Log.i("DjinnVisualsTest", "$style: ${(System.nanoTime() - started) / 1_000_000 / FRAMES} ms a frame")
            lit = litShare()
            assertEquals("$style failures", emptyList<String>(), renderer.failures)
            assertEquals("$style GL error", GLES20.GL_NO_ERROR, GLES20.glGetError())
            assertTrue("$style drew almost nothing: $lit", lit > 0.01f)
        }
    }

    private companion object {
        const val FRAMES = 40
    }

    private fun feedSoundOnce(frame: Int) {
        if (frame == 0) feedSound()
        // A little more each frame, so the reader sees the stream moving on.
        repeat(735) { i ->
            val t = frame * 735 + i
            Spectrum.analyzer.add(0.5f * sin(2 * PI * 110 * t / 44100).toFloat(), 44100)
        }
    }
}
