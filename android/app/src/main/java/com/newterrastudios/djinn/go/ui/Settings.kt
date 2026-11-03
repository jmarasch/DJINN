package com.newterrastudios.djinn.go.ui

import android.content.Context
import android.text.format.Formatter
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Close
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Icon
import androidx.compose.material3.TextButton
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Slider
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.media3.common.util.UnstableApi
import com.newterrastudios.djinn.go.data.GoPrefs
import com.newterrastudios.djinn.go.data.KIND_DOWNLOAD
import com.newterrastudios.djinn.go.data.KIND_SMART
import com.newterrastudios.djinn.go.data.OfflineSong
import com.newterrastudios.djinn.go.player.Spectrum
import com.newterrastudios.djinn.go.work.SmartCacheWorker
import java.io.IOException
import kotlinx.coroutines.launch

private val QUALITIES = listOf(
    0 to "Full quality: keep the original files",
    96 to "MP3, about 96 kbit/s",
    128 to "MP3, about 128 kbit/s",
    192 to "MP3, about 192 kbit/s",
    320 to "MP3, about 320 kbit/s",
)
private val SMART_LIMITS = listOf(512, 1024, 2048, 5120, 10240)

fun sizeText(context: Context, bytes: Long): String = Formatter.formatShortFileSize(context, bytes)

@Composable
internal fun Toggle(title: String, note: String?, on: Boolean, change: (Boolean) -> Unit) {
    Row(
        Modifier
            .fillMaxWidth()
            .clickable { change(!on) }
            .padding(horizontal = 16.dp, vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Column(Modifier.weight(1f)) {
            Text(title)
            if (note != null) {
                Text(note, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
        }
        Switch(checked = on, onCheckedChange = change)
    }
}

@UnstableApi
@Composable
fun SettingsScreen(nav: Nav) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val prefs = nav.prefs
    val server = nav.server
    val kept by remember(server) { nav.offline.dao.songsFlow(server) }.collectAsState(emptyList())
    val sounds by remember(server) { nav.offline.dao.soundsFlow(server) }.collectAsState(emptyList())
    var kbps by remember { mutableIntStateOf(prefs.kbps) }
    var smart by remember { mutableStateOf(prefs.smartCache) }
    var limit by remember { mutableIntStateOf(prefs.smartLimitMb) }
    var wifi by remember { mutableStateOf(prefs.wifiOnly) }
    val downloads = kept.filter { it.kind == KIND_DOWNLOAD }
    val smartSongs = kept.filter { it.kind == KIND_SMART }
    var confirmSignOut by remember { mutableStateOf(false) }

    if (confirmSignOut) {
        AlertDialog(
            onDismissRequest = { confirmSignOut = false },
            title = { Text("Sign out?") },
            text = { Text("The songs and system sounds kept on this phone will be deleted. You can download them again after signing back in.") },
            confirmButton = {
                TextButton(onClick = {
                    confirmSignOut = false
                    nav.signOut()
                }) { Text("Sign out and delete") }
            },
            dismissButton = { TextButton(onClick = { confirmSignOut = false }) { Text("Stay signed in") } },
        )
    }

    LazyColumn(Modifier.fillMaxSize()) {
        item {
            Text("Settings", style = MaterialTheme.typography.headlineSmall, modifier = Modifier.padding(16.dp))
            Text(
                "Signed in to " + (nav.serverName?.let { "$it ($server)" } ?: server),
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(horizontal = 16.dp),
            )
            OutlinedButton(
                onClick = { if (kept.isEmpty() && sounds.isEmpty()) nav.signOut() else confirmSignOut = true },
                modifier = Modifier.padding(16.dp),
            ) { Text("Sign out") }
        }
        item { TapSettings(nav) }
        item { VisualsSettings(nav) }
        if (nav.canKeepSongs) {
            item {
                Heading("Quality of kept songs")
                Note("FLAC and WAV songs can be converted, on your server, to take less room. Other formats stay as they are. Applies to songs fetched from now on.")
            }
            items(QUALITIES) { (rate, label) ->
                Row(
                    Modifier
                        .fillMaxWidth()
                        .clickable {
                            kbps = rate
                            prefs.kbps = rate
                        }
                        .padding(horizontal = 16.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    RadioButton(selected = kbps == rate, onClick = {
                        kbps = rate
                        prefs.kbps = rate
                    })
                    Text(label)
                }
            }
            item {
                Heading("Storage")
                DownloadStatus()
                Toggle("Download on Wi-Fi only", null, wifi) {
                    wifi = it
                    prefs.wifiOnly = it
                }
                Toggle(
                    "Smart cache",
                    "Keep what you play most and like, on Wi-Fi while charging.",
                    smart,
                ) {
                    smart = it
                    prefs.smartCache = it
                    SmartCacheWorker.runNow(context)
                }
                if (smart) {
                    Row(
                        Modifier
                            .fillMaxWidth()
                            .padding(horizontal = 16.dp),
                        horizontalArrangement = Arrangement.spacedBy(4.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Text("Up to", modifier = Modifier.padding(end = 4.dp))
                        SMART_LIMITS.forEach { mb ->
                            val selected = limit == mb
                            Text(
                                if (mb < 1024) "$mb MB" else "${mb / 1024} GB",
                                style = MaterialTheme.typography.bodySmall,
                                color = if (selected) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant,
                                modifier = Modifier
                                    .clickable {
                                        limit = mb
                                        prefs.smartLimitMb = mb
                                    }
                                    .padding(8.dp),
                            )
                        }
                    }
                    OutlinedButton(
                        onClick = { SmartCacheWorker.runNow(context) },
                        modifier = Modifier.padding(horizontal = 16.dp),
                    ) { Text("Update smart cache now") }
                }
                Note(usage(context, downloads, smartSongs))
                if (kbps > 0 && kept.any { !it.converted && it.path.endsWith(".flac") || !it.converted && it.path.endsWith(".wav") }) {
                    Note("Some lossless songs are at full size: your server isn't converting for this account. Admin > GO can turn converting on.")
                }
                if (kept.isNotEmpty()) {
                    OutlinedButton(
                        onClick = { scope.launch { nav.offline.deleteEverything() } },
                        modifier = Modifier.padding(horizontal = 16.dp),
                    ) { Text("Remove all kept songs and sounds") }
                }
            }
            item {
                Heading("Saved system sounds")
                if (sounds.isEmpty()) {
                    Note("A song's menu can save it as an alarm, ringtone or notification sound.")
                }
            }
            items(sounds, key = { it.uri }) { sound ->
                Row2(
                    title = sound.title,
                    subtitle = sound.kind,
                    art = {},
                    onClick = {},
                    end = {
                        IconButton(onClick = { scope.launch { nav.offline.removeSound(sound) } }) {
                            Icon(Icons.Default.Close, "Remove ${sound.title}")
                        }
                    },
                )
            }
        } else {
            item { Note("Your role doesn't include keeping songs on this phone. Ask your host for the download permission.") }
        }
    }
}

private val TAP_CHOICES = listOf(
    "play" to "Play it now",
    "queue" to "Play it next",
    "end" to "Add it to the end of the queue",
)

/** What a tap on a song does. Kept on the account, so every device follows it. */
@UnstableApi
@Composable
private fun TapSettings(nav: Nav) {
    Heading("When you tap a song")
    TAP_CHOICES.forEach { (value, label) ->
        fun choose() {
            val before = nav.tapAction
            nav.tapAction = value
            nav.scope.launch {
                try {
                    nav.api.setPrefs(GoPrefs(value))
                } catch (_: IOException) {
                    nav.tapAction = before
                    nav.message = "Couldn't reach the server, so that wasn't changed."
                }
            }
        }
        Row(
            Modifier
                .fillMaxWidth()
                .clickable(onClick = ::choose)
                .padding(horizontal = 16.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            RadioButton(selected = nav.tapAction == value, onClick = ::choose)
            Text(label)
        }
    }
}

/** The visuals' timing and strength. Both take effect as they're moved. */
@UnstableApi
@Composable
private fun VisualsSettings(nav: Nav) {
    val prefs = nav.prefs
    var lead by remember { mutableFloatStateOf(prefs.visualsLeadMs.toFloat()) }
    var intensity by remember { mutableFloatStateOf(prefs.visualsIntensity) }
    var auto by remember { mutableStateOf(prefs.visualsAuto) }
    Heading("Visuals")
    Note("The sparkle button on Now Playing switches between the cover and these. Fancier styles use more battery.")
    VisualLookSettings(nav)
    Toggle("Change styles by themselves", "In full screen, a new style every 30 seconds.", auto) {
        auto = it
        prefs.visualsAuto = it
    }
    Column(Modifier.padding(horizontal = 16.dp)) {
        Text("Timing: ${lead.toInt()} ms")
        Text(
            "If visuals seem to come before the sound, raise this; if after, lower it.",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Slider(
            value = lead,
            onValueChange = {
                lead = it
                prefs.visualsLeadMs = it.toInt()
                Spectrum.leadMs = it.toInt()
            },
            valueRange = 0f..800f,
            steps = 15,
        )
        Text("Strength: ${(intensity * 100).toInt()}%")
        Slider(
            value = intensity,
            onValueChange = {
                intensity = it
                prefs.visualsIntensity = it
                Spectrum.intensity = it
            },
            valueRange = 0.5f..1.5f,
        )
    }
}

private fun usage(context: Context, downloads: List<OfflineSong>, smart: List<OfflineSong>): String =
    "Downloads: ${songCount(downloads.size)}, ${sizeText(context, downloads.sumOf { it.bytes })}. " +
        "Smart cache: ${songCount(smart.size)}, ${sizeText(context, smart.sumOf { it.bytes })}."

private fun songCount(n: Int) = if (n == 1) "1 song" else "$n songs"
