package com.newterrastudios.djinn.go.ui

import android.app.ActivityManager
import android.content.Context
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.FilterChip
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Slider
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp
import androidx.media3.common.util.UnstableApi
import com.newterrastudios.djinn.go.player.BANDS
import com.newterrastudios.djinn.go.player.COLORS_APP
import com.newterrastudios.djinn.go.player.COLORS_CUSTOM
import com.newterrastudios.djinn.go.player.COLORS_ROTATE
import com.newterrastudios.djinn.go.player.Spectrum
import com.newterrastudios.djinn.go.player.VisualLook
import com.newterrastudios.djinn.go.i18n.t

/**
 * Whether to offer visuals at all. Only the plain facts can be asked up front;
 * how each style actually runs is watched while it plays (`FrameWatch`).
 */
fun canShowVisuals(context: Context): Boolean {
    val activity = context.getSystemService(ActivityManager::class.java) ?: return false
    return !activity.isLowRamDevice && activity.deviceConfigurationInfo.reqGlEsVersion >= 0x20000
}

private val SWATCHES = listOf(
    0xB48CFF, 0x4F7BFF, 0x22D3EE, 0x34D399, 0xFACC15, 0xFB923C, 0xF43F5E, 0xF472B6, 0xFFFFFF,
)

private val MODES = listOf(
    COLORS_APP to "visual.app_colors",
    COLORS_CUSTOM to "visual.your_colors",
    COLORS_ROTATE to "visual.rgb_rotation",
)

/** Picks a style, its colors and its setup, with the result showing above them. */
@UnstableApi
@Composable
fun VisualLookSettings(nav: Nav) {
    var look by remember { mutableStateOf(Spectrum.look) }
    var style by remember { mutableStateOf(nav.currentStyle()) }
    fun change(next: VisualLook) {
        look = next
        Spectrum.look = next
        nav.prefs.visualLook = next
    }
    Column(Modifier.padding(horizontal = 16.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
        Visual(
            style,
            onTap = {},
            onTooSlow = nav::styleTooSlow,
            modifier = Modifier
                .fillMaxWidth()
                .height(140.dp)
                .clip(MaterialTheme.shapes.medium)
                .background(Color.Black),
            onBlack = true,
        )
        Text(
            t("visual.moves_note"),
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Row(
            Modifier.horizontalScroll(rememberScrollState()),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            nav.visualStyles.forEach { each ->
                FilterChip(
                    selected = each == style,
                    onClick = {
                        style = each
                        nav.prefs.visuals = each.ordinal
                    },
                    label = { Text(each.label) },
                )
            }
        }
        if (nav.slowStyles.isNotEmpty()) {
            TextButton(onClick = nav::retrySlowStyles) {
                Text(t("visual.too_slow", "n" to nav.slowStyles.size))
            }
        }

        Text(t("visual.colors"), style = MaterialTheme.typography.titleSmall, modifier = Modifier.padding(top = 12.dp))
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            MODES.forEach { (mode, label) ->
                FilterChip(
                    selected = look.colors == mode,
                    onClick = { change(look.copy(colors = mode)) },
                    label = { Text(t(label)) },
                )
            }
        }
        when (look.colors) {
            COLORS_CUSTOM -> {
                ColorChoice(t("visual.quiet"), look.quiet) { change(look.copy(quiet = it)) }
                ColorChoice(t("visual.loud"), look.loud) { change(look.copy(loud = it)) }
            }
            COLORS_ROTATE -> Level(
                t("visual.rotate_every", "n" to look.rotateSeconds.toInt()),
                look.rotateSeconds,
                3f..60f,
            ) { change(look.copy(rotateSeconds = it)) }
        }

        Text(t("visual.bars_lines"), style = MaterialTheme.typography.titleSmall, modifier = Modifier.padding(top = 12.dp))
        Level(t("visual.bar_count", "n" to look.bars), look.bars.toFloat(), 8f..BANDS.toFloat()) {
            change(look.copy(bars = it.toInt()))
        }
        Level("Space between bars: ${(look.gap * 100).toInt()}%", look.gap, 0f..0.7f) {
            change(look.copy(gap = it))
        }
        Level(t("visual.line_width", "n" to look.lineWidth.toInt()), look.lineWidth, 1f..16f) {
            change(look.copy(lineWidth = it))
        }
        Level("Smoothing: ${(look.calm * 100).toInt()}%", look.calm, 0f..1f) {
            change(look.copy(calm = it))
        }
        Row(
            Modifier
                .fillMaxWidth()
                .clickable { change(look.copy(rounded = !look.rounded)) }
                .padding(vertical = 4.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(t("visual.rounded"), modifier = Modifier.weight(1f))
            androidx.compose.material3.Switch(
                checked = look.rounded,
                onCheckedChange = { change(look.copy(rounded = it)) },
            )
        }
        OutlinedButton(onClick = { change(VisualLook()) }) { Text(t("visual.defaults")) }
    }
}

@Composable
private fun Level(label: String, value: Float, range: ClosedFloatingPointRange<Float>, change: (Float) -> Unit) {
    Text(label, style = MaterialTheme.typography.bodyMedium)
    Slider(value = value, onValueChange = change, valueRange = range)
}

/** Ready-made colors, and exact ones for whoever wants them. */
@Composable
private fun ColorChoice(label: String, rgb: Int, change: (Int) -> Unit) {
    var exact by remember { mutableStateOf(false) }
    Row(
        Modifier
            .fillMaxWidth()
            .padding(top = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(label, modifier = Modifier.weight(1f))
        Box(
            Modifier
                .size(20.dp)
                .clip(CircleShape)
                .background(color(rgb)),
        )
        TextButton(onClick = { exact = !exact }) { Text(if (exact) t("common.done") else t("visual.exact")) }
    }
    Row(
        Modifier.horizontalScroll(rememberScrollState()),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        SWATCHES.forEach { swatch ->
            Box(
                Modifier
                    .size(36.dp)
                    .clip(CircleShape)
                    .background(color(swatch))
                    .border(
                        width = if (swatch == rgb) 3.dp else 0.dp,
                        color = MaterialTheme.colorScheme.onSurface,
                        shape = CircleShape,
                    )
                    .clickable { change(swatch) },
            )
        }
    }
    if (exact) {
        listOf("visual.red" to 16, "visual.green" to 8, "visual.blue" to 0).forEach { (key, shift) ->
            val amount = (rgb shr shift) and 0xFF
            Level("${t(key)}: $amount", amount.toFloat(), 0f..255f) {
                change((rgb and (0xFF shl shift).inv()) or (it.toInt() shl shift))
            }
        }
    }
}

/** 0xRRGGBB on screen. */
fun color(rgb: Int) = Color(0xFF000000L or (rgb.toLong() and 0xFFFFFFL))
