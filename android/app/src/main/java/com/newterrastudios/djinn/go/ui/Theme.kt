package com.newterrastudios.djinn.go.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Shapes
import androidx.compose.material3.Typography
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawOutline
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.Font
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import com.newterrastudios.djinn.go.R

// The same colors as the web app (web/src/app.css).
private val Dark = darkColorScheme(
    primary = Color(0xFFB48CFF),
    onPrimary = Color(0xFF1A1030),
    primaryContainer = Color(0xFF332D47),
    onPrimaryContainer = Color(0xFFECE8F6),
    secondary = Color(0xFF3FE0CF),
    secondaryContainer = Color(0xFF332D47),
    onSecondaryContainer = Color(0xFFECE8F6),
    background = Color(0xFF14111F),
    onBackground = Color(0xFFECE8F6),
    surface = Color(0xFF14111F),
    onSurface = Color(0xFFECE8F6),
    surfaceVariant = Color(0xFF272237),
    onSurfaceVariant = Color(0xFF9A93B3),
    surfaceContainer = Color(0xFF1C1829),
    surfaceContainerHigh = Color(0xFF272237),
    surfaceContainerLow = Color(0xFF1C1829),
    outline = Color(0xFF9A93B3),
    outlineVariant = Color(0xFF332D47),
)

private val Light = lightColorScheme(
    primary = Color(0xFF6F3FD6),
    onPrimary = Color(0xFFFFFFFF),
    primaryContainer = Color(0xFFDDD7EC),
    onPrimaryContainer = Color(0xFF1D1830),
    secondary = Color(0xFF0D9488),
    secondaryContainer = Color(0xFFDDD7EC),
    onSecondaryContainer = Color(0xFF1D1830),
    background = Color(0xFFF6F4FB),
    onBackground = Color(0xFF1D1830),
    surface = Color(0xFFF6F4FB),
    onSurface = Color(0xFF1D1830),
    surfaceVariant = Color(0xFFECE8F5),
    onSurfaceVariant = Color(0xFF6B6485),
    surfaceContainer = Color(0xFFFFFFFF),
    surfaceContainerHigh = Color(0xFFECE8F5),
    surfaceContainerLow = Color(0xFFFFFFFF),
    outline = Color(0xFF6B6485),
    outlineVariant = Color(0xFFDDD7EC),
)

/** The neon look: a thin lavender edge with a soft glow, as on the web (--edge, --glow). */
class Neon(
    val edge: Color,
    val edgeStrong: Color,
    val glow: Color,
    val cardTop: Color,
    val cardBottom: Color,
    val accent2: Color,
    val dark: Boolean,
)

private val DarkNeon = Neon(
    edge = Color(0xB8BE96FF),
    edgeStrong = Color(0xFFCDAAFF),
    glow = Color(0xFFB48CFF),
    cardTop = Color(0xD1272139),
    cardBottom = Color(0xE61A1627),
    accent2 = Color(0xFF3FE0CF),
    dark = true,
)

private val LightNeon = Neon(
    edge = Color(0x426F3FD6),
    edgeStrong = Color(0x996F3FD6),
    glow = Color(0xFF6F3FD6),
    cardTop = Color(0xFFFFFFFF),
    cardBottom = Color(0xFFFFFFFF),
    accent2 = Color(0xFF0D9488),
    dark = false,
)

val LocalNeon = staticCompositionLocalOf { DarkNeon }

private val Outfit = FontFamily(
    Font(R.font.outfit_regular, FontWeight.Normal),
    Font(R.font.outfit_medium, FontWeight.Medium),
    Font(R.font.outfit_semibold, FontWeight.SemiBold),
    Font(R.font.outfit_bold, FontWeight.Bold),
)

private fun TextStyle.outfit() = copy(fontFamily = Outfit)

private val Base = Typography()
private val DjinnTypography = Typography(
    displayLarge = Base.displayLarge.outfit(),
    displayMedium = Base.displayMedium.outfit(),
    displaySmall = Base.displaySmall.outfit(),
    headlineLarge = Base.headlineLarge.outfit(),
    headlineMedium = Base.headlineMedium.outfit(),
    headlineSmall = Base.headlineSmall.outfit(),
    titleLarge = Base.titleLarge.outfit(),
    titleMedium = Base.titleMedium.outfit(),
    titleSmall = Base.titleSmall.outfit(),
    bodyLarge = Base.bodyLarge.outfit(),
    bodyMedium = Base.bodyMedium.outfit(),
    bodySmall = Base.bodySmall.outfit(),
    labelLarge = Base.labelLarge.outfit(),
    labelMedium = Base.labelMedium.outfit(),
    labelSmall = Base.labelSmall.outfit(),
)

private val DjinnShapes = Shapes(
    extraSmall = RoundedCornerShape(6.dp),
    small = RoundedCornerShape(10.dp),
    medium = RoundedCornerShape(14.dp),
    large = RoundedCornerShape(18.dp),
    extraLarge = RoundedCornerShape(24.dp),
)

@Composable
fun DjinnTheme(content: @Composable () -> Unit) {
    val dark = isSystemInDarkTheme()
    CompositionLocalProvider(LocalNeon provides if (dark) DarkNeon else LightNeon) {
        MaterialTheme(
            colorScheme = if (dark) Dark else Light,
            typography = DjinnTypography,
            shapes = DjinnShapes,
        ) {
            // The page's own backdrop (--page-bg), behind every screen.
            Box(Modifier.fillMaxSize().djinnBackdrop(dark)) { content() }
        }
    }
}

/** The soft purple light in two corners of the page, over the base color. */
private fun Modifier.djinnBackdrop(dark: Boolean): Modifier =
    if (!dark) {
        background(Color(0xFFF6F4FB))
    } else {
        background(Color(0xFF14111F)).drawBehind {
            drawRect(
                Brush.radialGradient(
                    listOf(Color(0x38 shl 24 or 0x7C40C8), Color.Transparent),
                    center = Offset(size.width * 0.85f, -size.height * 0.05f),
                    radius = size.width * 1.0f,
                ),
            )
            drawRect(
                Brush.radialGradient(
                    listOf(Color(0x33 shl 24 or 0x5A2896), Color.Transparent),
                    center = Offset(-size.width * 0.1f, size.height * 1.05f),
                    radius = size.width * 0.9f,
                ),
            )
        }
    }

/**
 * A neon-edged panel: the card gradient, a thin lavender line and a soft glow
 * around it. [strong] for something hovered, playing or chosen.
 */
@Composable
fun Modifier.neon(shape: Shape = RoundedCornerShape(14.dp), strong: Boolean = false): Modifier {
    val neon = LocalNeon.current
    val glow = neon.glow
    val density = androidx.compose.ui.platform.LocalDensity.current
    val direction = androidx.compose.ui.platform.LocalLayoutDirection.current
    return this
        // The glow: a few soft rings spreading outward from the edge.
        .drawBehind {
            val outline = shape.createOutline(size, direction, density)
            val rings = if (strong) listOf(14f to 0.10f, 9f to 0.16f, 5f to 0.24f, 2.5f to 0.36f) else listOf(10f to 0.08f, 6f to 0.14f, 3f to 0.22f)
            rings.forEach { (width, alpha) ->
                drawOutline(outline, glow.copy(alpha = alpha), style = Stroke(width.dp.toPx() * 2))
            }
        }
        .background(Brush.verticalGradient(listOf(neon.cardTop, neon.cardBottom)), shape)
        .border(1.dp, if (strong) neon.edgeStrong else neon.edge, shape)
}
