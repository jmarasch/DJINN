package com.newterrastudios.djinn.go.ui

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color

// The same colors as the web app (web/src/app.css).
private val Dark = darkColorScheme(
    primary = Color(0xFFB48CFF),
    onPrimary = Color(0xFF1A1030),
    primaryContainer = Color(0xFF332D47),
    onPrimaryContainer = Color(0xFFECE8F6),
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

@Composable
fun DjinnTheme(content: @Composable () -> Unit) {
    MaterialTheme(colorScheme = if (isSystemInDarkTheme()) Dark else Light, content = content)
}
