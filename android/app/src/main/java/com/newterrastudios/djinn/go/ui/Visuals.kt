package com.newterrastudios.djinn.go.ui

import android.app.Activity
import androidx.activity.compose.BackHandler
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.layout.size
import androidx.compose.material.icons.filled.Album
import androidx.compose.material3.Slider
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.ui.layout.ContentScale
import coil3.compose.AsyncImage
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.AutoAwesome
import androidx.compose.material.icons.filled.ChevronLeft
import androidx.compose.material.icons.filled.ChevronRight
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Pause
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.filled.SkipNext
import androidx.compose.material.icons.filled.SkipPrevious
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalLifecycleOwner
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.media3.common.util.UnstableApi
import com.newterrastudios.djinn.go.player.COLORS_CUSTOM
import com.newterrastudios.djinn.go.player.COLORS_ROTATE
import com.newterrastudios.djinn.go.player.binBands
import com.newterrastudios.djinn.go.player.hueToRgb
import com.newterrastudios.djinn.go.player.rotatedHue
import com.newterrastudios.djinn.go.player.SoundReader
import com.newterrastudios.djinn.go.player.Spectrum
import com.newterrastudios.djinn.go.player.WAVE_POINTS
import com.newterrastudios.djinn.go.i18n.t
import kotlinx.coroutines.delay
import kotlin.math.PI
import kotlin.math.cos
import kotlin.math.min
import kotlin.math.sin

/** Stored by position in Settings, so new ones go on the end. */
enum class VisualStyle {
    Art,
    Bars,
    Ring,
    Wave,
    Mirror,
    Warp,
    Tunnel,
    Plasma,
    Kaleidoscope,
    Nebula,
    Stars,
    ;

    /** The style's name in the phone's language. */
    val label: String get() = t("visual.${name.lowercase()}")

    /** Drawn on the graphics chip, rather than as shapes. */
    val onGraphicsChip get() = fragmentFor(this) != null

    companion object {
        /** Every style that shows the sound. */
        val moving = entries.filter { it != Art }

        fun at(index: Int) = entries.getOrElse(index) { Art }
    }
}

/** How long full screen stays on one style when set to change by itself. */
private const val AUTO_CHANGE_MS = 30_000L
private const val CONTROLS_MS = 4_000L
private const val TITLE_MS = 6_000L

/** Any moving style, sized by its caller. A tap reaches `onTap`. */
@UnstableApi
@Composable
fun Visual(
    style: VisualStyle,
    onTap: () -> Unit,
    modifier: Modifier = Modifier,
    onBlack: Boolean = false,
    onTooSlow: (VisualStyle) -> Unit = {},
) {
    DisposableEffect(Unit) {
        Spectrum.watch()
        onDispose { Spectrum.unwatch() }
    }
    Box(modifier) {
        if (style.onGraphicsChip) {
            ChipVisual(style, onTooSlow, Modifier.fillMaxSize())
        } else {
            ShapeVisual(style, onBlack, Modifier.fillMaxSize())
        }
        // On top, since the chip's surface would otherwise take the touches.
        Box(
            Modifier
                .fillMaxSize()
                .clickable(
                    interactionSource = remember { MutableInteractionSource() },
                    indication = null,
                    onClick = onTap,
                ),
        )
    }
}

@UnstableApi
@Composable
private fun ChipVisual(style: VisualStyle, onTooSlow: (VisualStyle) -> Unit, modifier: Modifier) {
    val context = LocalContext.current
    val owner = LocalLifecycleOwner.current
    val view = remember { VisualsView(context, style) }
    val tooSlow by rememberUpdatedState(onTooSlow)
    DisposableEffect(view) {
        view.visuals.onTooSlow = { slow -> view.post { tooSlow(slow) } }
        onDispose { view.visuals.onTooSlow = null }
    }
    DisposableEffect(owner) {
        // Stops drawing when the app isn't showing; a running shader eats battery.
        val watcher = LifecycleEventObserver { _, event ->
            when (event) {
                Lifecycle.Event.ON_RESUME -> view.onResume()
                Lifecycle.Event.ON_PAUSE -> view.onPause()
                else -> Unit
            }
        }
        owner.lifecycle.addObserver(watcher)
        onDispose {
            owner.lifecycle.removeObserver(watcher)
            view.onPause()
        }
    }
    AndroidView(factory = { view }, update = { it.visuals.style = style }, modifier = modifier)
}

@UnstableApi
@Composable
private fun ShapeVisual(style: VisualStyle, onBlack: Boolean, modifier: Modifier) {
    val sound = remember { SoundReader(Spectrum.analyzer) }
    var frame by remember { mutableLongStateOf(0L) }
    LaunchedEffect(Unit) {
        while (true) {
            withFrameNanos { now ->
                sound.read(now, Spectrum.leadMs, Spectrum.intensity, Spectrum.look.calm)
                frame = now
            }
        }
    }
    // Full screen is always black, whatever the phone's theme.
    val low = if (onBlack) Color(0xFFB48CFF) else MaterialTheme.colorScheme.primary
    val high = if (onBlack) Color.White else MaterialTheme.colorScheme.onSurface
    Canvas(modifier) {
        // Reading it redraws each frame.
        frame.let { }
        val look = Spectrum.look
        val seconds = frame / 1_000_000_000.0
        val barCount = look.bars
        // Music keeps most bands well up the scale; squaring gives the bars room to move.
        val bands = binBands(sound.bands, barCount).let { bars -> FloatArray(bars.size) { bars[it] * bars[it] } }
        // `position` is where along the picture this is, for RGB rotation to spread across.
        fun paint(position: Float, level: Float): Color = when (look.colors) {
            COLORS_ROTATE -> lerp(color(hueToRgb(rotatedHue(seconds, look.rotateSeconds, position))), Color.White, level * 0.6f)
            COLORS_CUSTOM -> lerp(color(look.quiet), color(look.loud), level)
            else -> lerp(low, high, level)
        }
        val corner = { width: Float -> CornerRadius(if (look.rounded) width / 2 else 0f) }
        val gapShare = look.gap.coerceIn(0f, 0.9f)
        when (style) {
            VisualStyle.Ring -> {
                val center = Offset(size.width / 2, size.height / 2)
                val inner = min(size.width, size.height) * (0.22f + 0.04f * sound.beat)
                val longest = min(size.width, size.height) * 0.26f
                // Mirrored, so the two sides meet without a jump.
                val spokes = barCount * 2
                for (i in 0 until spokes) {
                    val band = if (i < barCount) i else spokes - 1 - i
                    val angle = 2 * PI * i / spokes - PI / 2
                    val length = inner + 2f + longest * bands[band]
                    drawLine(
                        color = paint(band.toFloat() / barCount, bands[band]),
                        start = center + Offset((cos(angle) * inner).toFloat(), (sin(angle) * inner).toFloat()),
                        end = center + Offset((cos(angle) * length).toFloat(), (sin(angle) * length).toFloat()),
                        strokeWidth = (2 * PI.toFloat() * inner / spokes) * (1f - gapShare),
                        cap = if (look.rounded) StrokeCap.Round else StrokeCap.Butt,
                    )
                }
            }
            VisualStyle.Wave -> {
                val path = Path()
                val middle = size.height / 2
                val reach = size.height * 0.45f
                for (i in 0 until WAVE_POINTS) {
                    val x = size.width * i / (WAVE_POINTS - 1)
                    val y = middle - sound.wave[i] * reach
                    if (i == 0) path.moveTo(x, y) else path.lineTo(x, y)
                }
                drawPath(
                    path,
                    paint(0f, 0.5f + sound.beat / 2),
                    style = Stroke(width = look.lineWidth.dp.toPx() * (1f + 1.5f * sound.bass), cap = StrokeCap.Round),
                )
            }
            VisualStyle.Mirror -> {
                val gap = size.width / barCount * gapShare
                val width = size.width / barCount - gap
                val middle = size.height / 2
                for (i in 0 until barCount) {
                    val height = (size.height / 2 * bands[i]).coerceAtLeast(width / 2)
                    drawRoundRect(
                        color = paint(i.toFloat() / barCount, bands[i]),
                        topLeft = Offset(i * (width + gap) + gap / 2, middle - height),
                        size = Size(width, height * 2),
                        cornerRadius = corner(width),
                    )
                }
            }
            else -> {
                val gap = size.width / barCount * gapShare
                val width = size.width / barCount - gap
                for (i in 0 until barCount) {
                    val height = (size.height * bands[i]).coerceAtLeast(width)
                    drawRoundRect(
                        color = paint(i.toFloat() / barCount, bands[i]),
                        topLeft = Offset(i * (width + gap) + gap / 2, size.height - height),
                        size = Size(width, height),
                        cornerRadius = corner(width),
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

/** The biggest copy of a song's picture, rather than the size lists use. */
private fun fullSizeArt(url: String?): String? = url?.substringBefore("?size=")

/**
 * The cover or the visuals alone, filling the screen, which stays on. A tap
 * brings the controls back for a few seconds, and another puts them away; a new
 * song shows its name for a few.
 */
@UnstableApi
@Composable
fun FullScreen(nav: Nav, cover: Boolean) {
    val player = nav.player
    var style by remember { mutableStateOf(nav.currentStyle()) }
    var auto by remember { mutableStateOf(nav.prefs.visualsAuto) }
    var controls by remember { mutableStateOf(true) }
    var touched by remember { mutableIntStateOf(0) }
    var showTitle by remember { mutableStateOf(true) }
    var dragging by remember { mutableStateOf<Float?>(null) }

    BackHandler { nav.pop() }
    val view = LocalView.current
    DisposableEffect(Unit) {
        val window = (view.context as? Activity)?.window
        val bars = window?.let { WindowCompat.getInsetsController(it, view) }
        bars?.systemBarsBehavior = WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
        bars?.hide(WindowInsetsCompat.Type.systemBars())
        view.keepScreenOn = true
        onDispose {
            bars?.show(WindowInsetsCompat.Type.systemBars())
            view.keepScreenOn = false
        }
    }
    fun change(by: Int) {
        val styles = nav.visualStyles
        val next = styles[(styles.indexOf(style) + by + styles.size) % styles.size]
        style = next
        nav.prefs.visuals = next.ordinal
    }
    LaunchedEffect(touched) {
        controls = true
        delay(CONTROLS_MS)
        controls = false
    }
    LaunchedEffect(auto) {
        while (auto && !cover) {
            delay(AUTO_CHANGE_MS)
            change(1)
        }
    }
    LaunchedEffect(player.current?.mediaId) {
        showTitle = true
        delay(TITLE_MS)
        showTitle = false
    }

    Box(
        Modifier
            .fillMaxSize()
            .background(Color.Black),
    ) {
        fun tapped() {
            if (controls) controls = false else touched++
        }
        if (cover) {
            val art = player.current?.mediaMetadata?.artworkUri?.toString()
            Icon(
                Icons.Default.Album,
                contentDescription = null,
                tint = Color.White.copy(alpha = 0.25f),
                modifier = Modifier
                    .align(Alignment.Center)
                    .size(160.dp),
            )
            // The list-sized copy first, which is there even away from the server.
            listOf(art, fullSizeArt(art)).forEach { url ->
                AsyncImage(
                    model = url,
                    contentDescription = null,
                    contentScale = ContentScale.Fit,
                    modifier = Modifier.fillMaxSize(),
                )
            }
            Box(
                Modifier
                    .fillMaxSize()
                    .clickable(
                        interactionSource = remember { MutableInteractionSource() },
                        indication = null,
                        onClick = ::tapped,
                    ),
            )
        } else {
            Visual(
                style,
                onTap = ::tapped,
                modifier = Modifier.fillMaxSize(),
                onBlack = true,
                onTooSlow = { slow ->
                    nav.styleTooSlow(slow)
                    if (slow == style) style = nav.currentStyle()
                },
            )
        }
        AnimatedVisibility(
            visible = showTitle || controls,
            enter = fadeIn(),
            exit = fadeOut(),
            modifier = Modifier
                .align(Alignment.TopCenter)
                .safeDrawingPadding()
                .padding(top = 24.dp),
        ) {
            Column(horizontalAlignment = Alignment.CenterHorizontally) {
                Text(
                    player.current?.mediaMetadata?.title?.toString() ?: "",
                    color = Color.White,
                    style = MaterialTheme.typography.titleLarge,
                    textAlign = TextAlign.Center,
                )
                Text(
                    player.current?.mediaMetadata?.artist?.toString() ?: "",
                    color = Color.White.copy(alpha = 0.75f),
                    textAlign = TextAlign.Center,
                )
            }
        }
        AnimatedVisibility(
            visible = controls,
            enter = fadeIn(),
            exit = fadeOut(),
            modifier = Modifier
                .align(Alignment.BottomCenter)
                .fillMaxWidth()
                .safeDrawingPadding(),
        ) {
            Column(
                Modifier
                    .fillMaxWidth()
                    .background(Color.Black.copy(alpha = 0.45f))
                    .padding(8.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                if (!cover) {
                    Text(if (auto) t("visual.auto_named", "name" to style.label) else style.label, color = Color.White)
                }
                if (player.durationMs > 0) {
                    Slider(
                        value = dragging ?: player.positionMs.toFloat(),
                        onValueChange = {
                            dragging = it
                            touched++
                        },
                        onValueChangeFinished = {
                            dragging?.let { player.seekTo(it.toLong()) }
                            dragging = null
                        },
                        valueRange = 0f..player.durationMs.toFloat(),
                        modifier = Modifier.padding(horizontal = 8.dp),
                    )
                }
                Row(
                    Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.SpaceEvenly,
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    val tint = Color.White
                    IconButton(onClick = nav::pop) { Icon(Icons.Default.Close, t("visual.leave"), tint = tint) }
                    if (!cover) {
                        IconButton(onClick = {
                            change(-1)
                            touched++
                        }) { Icon(Icons.Default.ChevronLeft, t("visual.previous_style"), tint = tint) }
                    }
                    IconButton(onClick = {
                        player.previous()
                        touched++
                    }) { Icon(Icons.Default.SkipPrevious, t("visual.previous_song"), tint = tint) }
                    IconButton(onClick = {
                        player.toggle()
                        touched++
                    }) {
                        Icon(
                            if (player.playing) Icons.Default.Pause else Icons.Default.PlayArrow,
                            if (player.playing) t("player.pause") else t("player.play"),
                            tint = tint,
                        )
                    }
                    IconButton(onClick = {
                        player.next()
                        touched++
                    }, enabled = player.upNext.isNotEmpty()) {
                        Icon(Icons.Default.SkipNext, t("visual.next_song"), tint = tint.copy(alpha = if (player.upNext.isNotEmpty()) 1f else 0.4f))
                    }
                    if (!cover) {
                        IconButton(onClick = {
                            change(1)
                            touched++
                        }) { Icon(Icons.Default.ChevronRight, t("visual.next_style"), tint = tint) }
                        IconButton(onClick = {
                            auto = !auto
                            nav.prefs.visualsAuto = auto
                            touched++
                        }) {
                            Icon(
                                Icons.Default.AutoAwesome,
                                if (auto) t("visual.stop_auto") else t("visual.start_auto"),
                                tint = if (auto) MaterialTheme.colorScheme.primary else tint,
                            )
                        }
                    }
                }
            }
        }
    }
}
