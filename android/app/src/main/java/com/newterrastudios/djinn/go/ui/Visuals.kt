package com.newterrastudios.djinn.go.ui

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.clickable
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.StrokeCap
import androidx.media3.common.util.UnstableApi
import com.newterrastudios.djinn.go.player.BANDS
import com.newterrastudios.djinn.go.player.Spectrum
import kotlin.math.PI
import kotlin.math.cos
import kotlin.math.min
import kotlin.math.sin

/** Bars rise at once and fall gently, the way level meters do. */
private const val FALL_PER_FRAME = 0.025f

enum class VisualStyle { Art, Bars, Ring }

/** The song's sound, drawn from the audio on its way to the speaker. */
@UnstableApi
@Composable
fun Visualizer(style: VisualStyle, onTap: () -> Unit, modifier: Modifier = Modifier) {
    val shown = remember { FloatArray(BANDS) }
    var frame by remember { mutableLongStateOf(0L) }
    DisposableEffect(Unit) {
        Spectrum.listening = true
        onDispose {
            Spectrum.listening = false
            Spectrum.analyzer.clear()
        }
    }
    LaunchedEffect(Unit) {
        while (true) {
            withFrameNanos { now ->
                val bands = Spectrum.analyzer.bands
                for (i in 0 until BANDS) {
                    shown[i] = maxOf(bands[i], shown[i] - FALL_PER_FRAME)
                }
                frame = now
            }
        }
    }
    val low = MaterialTheme.colorScheme.primary
    val high = MaterialTheme.colorScheme.onSurface
    Canvas(modifier.clickable(onClick = onTap)) {
        // Reading it redraws each frame.
        frame.let { }
        when (style) {
            VisualStyle.Ring -> {
                val center = Offset(size.width / 2, size.height / 2)
                val inner = min(size.width, size.height) * 0.22f
                val longest = min(size.width, size.height) * 0.26f
                // Mirrored, so the two sides meet without a jump.
                val spokes = BANDS * 2
                for (i in 0 until spokes) {
                    val band = if (i < BANDS) i else spokes - 1 - i
                    val angle = 2 * PI * i / spokes - PI / 2
                    val length = inner + 2f + longest * shown[band]
                    drawLine(
                        color = lerp(low, high, shown[band]),
                        start = center + Offset((cos(angle) * inner).toFloat(), (sin(angle) * inner).toFloat()),
                        end = center + Offset((cos(angle) * length).toFloat(), (sin(angle) * length).toFloat()),
                        strokeWidth = (2 * PI.toFloat() * inner / spokes) * 0.6f,
                        cap = StrokeCap.Round,
                    )
                }
            }
            else -> {
                val gap = size.width / BANDS * 0.25f
                val width = size.width / BANDS - gap
                for (i in 0 until BANDS) {
                    val height = (size.height * shown[i]).coerceAtLeast(width)
                    drawRoundRect(
                        color = lerp(low, high, shown[i]),
                        topLeft = Offset(i * (width + gap) + gap / 2, size.height - height),
                        size = Size(width, height),
                        cornerRadius = CornerRadius(width / 2),
                    )
                }
            }
        }
    }
}

private fun lerp(a: Color, b: Color, t: Float): Color {
    val amount = (t * t).coerceIn(0f, 1f)
    return Color(
        a.red + (b.red - a.red) * amount,
        a.green + (b.green - a.green) * amount,
        a.blue + (b.blue - a.blue) * amount,
    )
}
