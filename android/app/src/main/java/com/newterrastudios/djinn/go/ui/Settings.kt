package com.newterrastudios.djinn.go.ui

import com.newterrastudios.djinn.go.data.ApiException
import com.newterrastudios.djinn.go.data.AppRelease
import com.newterrastudios.djinn.go.data.shouldAskAboutUpdate
import com.newterrastudios.djinn.go.player.TalkOver
import com.newterrastudios.djinn.go.BuildConfig
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
import androidx.compose.runtime.LaunchedEffect
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
import com.newterrastudios.djinn.go.i18n.t
import java.io.IOException
import kotlinx.coroutines.launch

/** Bit rates for converted songs; 0 keeps the original files. */
private val QUALITIES = listOf(0, 96, 128, 192, 320)

private fun qualityLabel(kbps: Int) =
    if (kbps == 0) t("settings.full_quality") else t("settings.mp3_rate", "kbps" to kbps)
private val SMART_LIMITS = listOf(512, 1024, 2048, 5120, 10240)
/** Songs to fetch ahead from the queue; 0 is off. */
private val AHEAD_CHOICES = listOf(0, 1, 3, 5, 10)

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
    var elsewhere by remember { mutableStateOf(emptyList<OfflineSong>()) }
    LaunchedEffect(server) { elsewhere = nav.offline.keptElsewhere() }
    val kept by remember(server) { nav.offline.dao.songsFlow(server) }.collectAsState(emptyList())
    val sounds by remember(server) { nav.offline.dao.soundsFlow(server) }.collectAsState(emptyList())
    var kbps by remember { mutableIntStateOf(prefs.kbps) }
    var smart by remember { mutableStateOf(prefs.smartCache) }
    var limit by remember { mutableIntStateOf(prefs.smartLimitMb) }
    var wifi by remember { mutableStateOf(prefs.wifiOnly) }
    var ahead by remember { mutableIntStateOf(prefs.cacheAhead) }
    val downloads = kept.filter { it.kind == KIND_DOWNLOAD }
    val smartSongs = kept.filter { it.kind == KIND_SMART }
    var confirmSignOut by remember { mutableStateOf(false) }
    val keepsSongs = remember { prefs.keepRules.signOut == "keep" }

    if (confirmSignOut) {
        AlertDialog(
            onDismissRequest = { confirmSignOut = false },
            title = { Text(t("settings.sign_out_question")) },
            text = {
                Text(
                    if (keepsSongs) {
                        t("settings.sign_out_keeps")
                    } else {
                        t("settings.sign_out_deletes")
                    },
                )
            },
            confirmButton = {
                TextButton(onClick = {
                    confirmSignOut = false
                    nav.signOut()
                }) { Text(if (keepsSongs) t("settings.sign_out") else t("settings.sign_out_delete")) }
            },
            dismissButton = { TextButton(onClick = { confirmSignOut = false }) { Text(t("settings.stay_signed_in")) } },
        )
    }

    LazyColumn(Modifier.fillMaxSize()) {
        item {
            Text(t("settings.title"), style = MaterialTheme.typography.headlineSmall, modifier = Modifier.padding(16.dp))
            Text(
                t("settings.signed_in_to", "server" to (nav.serverName?.let { "$it (${nav.address})" } ?: nav.address)),
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(horizontal = 16.dp),
            )
            Note(
                when {
                    nav.securityCode != null -> t("signin.encrypted_code", "code" to (nav.securityCode ?: ""))
                    nav.encrypted -> t("signin.encrypted")
                    else -> t("settings.not_encrypted")
                },
            )
            Row(
                Modifier.padding(16.dp),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                OutlinedButton(onClick = nav.switchServer) { Text(t("settings.other_servers")) }
                OutlinedButton(
                    onClick = { if (kept.isEmpty() && sounds.isEmpty()) nav.signOut() else confirmSignOut = true },
                ) { Text(t("settings.sign_out")) }
            }
            if (elsewhere.isNotEmpty()) {
                Note(
                    t(
                        "settings.elsewhere",
                        "songs" to songCount(elsewhere.size),
                        "size" to sizeText(context, elsewhere.sumOf { it.bytes }),
                    ),
                )
                TextButton(
                    onClick = {
                        scope.launch {
                            nav.offline.deleteElsewhere()
                            elsewhere = emptyList()
                        }
                    },
                    modifier = Modifier.padding(horizontal = 8.dp),
                ) { Text(t("settings.delete_those")) }
            }
        }
        item { LanguageSettings(nav) }
        item {
            UpdateOffer(nav)
            var askUpdates by remember { mutableStateOf(prefs.askAboutUpdates) }
            Toggle(t("settings.ask_updates"), null, askUpdates) {
                askUpdates = it
                prefs.askAboutUpdates = it
            }
            Note(t("settings.app_version", "version" to BuildConfig.VERSION_NAME))
        }
        item { TapSettings(nav) }
        item { TalkOverSettings(nav) }
        item { VisualsSettings(nav) }
        item { EqualizerSettings(nav) }
        if (nav.canKeepSongs) {
            item {
                Heading(t("settings.quality"))
                Note(t("settings.quality_note"))
            }
            items(QUALITIES) { rate ->
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
                    Text(qualityLabel(rate))
                }
            }
            item {
                Heading(t("settings.storage"))
                DownloadStatus()
                Toggle(t("settings.wifi_only"), null, wifi) {
                    wifi = it
                    prefs.wifiOnly = it
                }
                Text(
                    t("settings.fetch_ahead"),
                    style = MaterialTheme.typography.bodyLarge,
                    modifier = Modifier.padding(start = 16.dp, end = 16.dp, top = 8.dp),
                )
                Row(
                    Modifier
                        .fillMaxWidth()
                        .padding(horizontal = 16.dp),
                    horizontalArrangement = Arrangement.spacedBy(4.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    AHEAD_CHOICES.forEach { n ->
                        val selected = ahead == n
                        Text(
                            if (n == 0) t("settings.fetch_ahead_off") else n.toString(),
                            style = MaterialTheme.typography.bodySmall,
                            color = if (selected) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant,
                            modifier = Modifier
                                .clickable {
                                    ahead = n
                                    prefs.cacheAhead = n
                                }
                                .padding(8.dp),
                        )
                    }
                }
                Note(t("settings.fetch_ahead_note"))
                Toggle(
                    t("settings.smart_cache"),
                    t("settings.smart_cache_note"),
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
                        Text(t("settings.up_to"), modifier = Modifier.padding(end = 4.dp))
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
                    ) { Text(t("settings.update_smart_cache")) }
                }
                Note(usage(context, downloads, smartSongs))
                if (kbps > 0 && kept.any { !it.converted && it.path.endsWith(".flac") || !it.converted && it.path.endsWith(".wav") }) {
                    Note(t("settings.not_converting"))
                }
                if (kept.isNotEmpty()) {
                    OutlinedButton(
                        onClick = { scope.launch { nav.offline.deleteEverything() } },
                        modifier = Modifier.padding(horizontal = 16.dp),
                    ) { Text(t("settings.remove_all")) }
                }
            }
            item {
                Heading(t("settings.saved_sounds"))
                if (sounds.isEmpty()) {
                    Note(t("settings.saved_sounds_note"))
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
                            Icon(Icons.Default.Close, t("signin.remove_named", "name" to sound.title))
                        }
                    },
                )
            }
        } else {
            item { Note(t("settings.no_downloads")) }
        }
    }
}

/** Fetches a new version and hands it to Android, which asks before installing. Says what went wrong, if anything. */
@UnstableApi
suspend fun Nav.installUpdate(release: AppRelease): String? = try {
    updater.install(updater.fetch(release))
    null
} catch (e: IOException) {
    if (e is ApiException) e.message else e.message ?: t("common.no_server")
}

/** Asks about a new version once when the app opens, unless that's turned off. */
@UnstableApi
@Composable
fun UpdatePrompt(nav: Nav) {
    val release = nav.update ?: return
    if (nav.askedAboutUpdate || !shouldAskAboutUpdate(release, nav.prefs.askAboutUpdates, nav.prefs.updateDeclined)) return
    var busy by remember { mutableStateOf(false) }
    var problem by remember { mutableStateOf<String?>(null) }
    AlertDialog(
        onDismissRequest = { nav.askedAboutUpdate = true },
        title = { Text(t("settings.update_question")) },
        text = {
            Text(problem ?: t("settings.update_available", "version" to release.version))
        },
        confirmButton = {
            TextButton(
                enabled = !busy,
                onClick = {
                    busy = true
                    problem = null
                    nav.scope.launch {
                        problem = nav.installUpdate(release)
                        busy = false
                        if (problem == null) nav.askedAboutUpdate = true
                    }
                },
            ) { Text(if (busy) t("settings.downloading") else t("settings.update")) }
        },
        dismissButton = {
            TextButton(onClick = {
                nav.prefs.updateDeclined = release.versionCode
                nav.askedAboutUpdate = true
            }) { Text(t("settings.not_now")) }
        },
    )
}

/** A newer version from the server, when there is one. Android asks before installing it. */
@UnstableApi
@Composable
fun UpdateOffer(nav: Nav) {
    val release = nav.update ?: return
    var busy by remember { mutableStateOf(false) }
    var problem by remember { mutableStateOf<String?>(null) }
    Column(Modifier.padding(horizontal = 16.dp, vertical = 8.dp)) {
        Text(t("settings.update_available", "version" to release.version))
        problem?.let { Text(it, color = MaterialTheme.colorScheme.error) }
        OutlinedButton(
            enabled = !busy,
            onClick = {
                busy = true
                problem = null
                nav.scope.launch {
                    problem = nav.installUpdate(release)
                    busy = false
                }
            },
        ) { Text(if (busy) t("settings.downloading") else t("settings.update")) }
    }
}

/** What a tap can do, as the server names it. */
private val TAP_CHOICES = listOf("play", "queue", "end")

/** What a tap on a song does. Kept on the account, so every device follows it. */
@UnstableApi
@Composable
private fun TapSettings(nav: Nav) {
    Heading(t("settings.when_tap"))
    TAP_CHOICES.forEach { value ->
        fun choose() {
            val before = nav.tapAction
            nav.tapAction = value
            nav.scope.launch {
                try {
                    nav.api.setPrefs(GoPrefs(value))
                } catch (_: IOException) {
                    nav.tapAction = before
                    nav.message = t("settings.not_changed")
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
            Text(t("settings.tap_$value"))
        }
    }
}

/** For a call being screened, directions, or a voice assistant. Answered calls always pause. */
@Composable
private fun TalkOverSettings(nav: Nav) {
    val chosen by nav.prefs.talkOverChoice.collectAsState()
    Heading(t("settings.talk_over"))
    Note(t("settings.talk_over_note"))
    TalkOver.entries.forEach { choice ->
        Row(
            Modifier
                .fillMaxWidth()
                .clickable { nav.prefs.chooseTalkOver(choice) }
                .padding(horizontal = 16.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            RadioButton(selected = chosen == choice, onClick = { nav.prefs.chooseTalkOver(choice) })
            Text(t("talkover.${choice.name.lowercase()}"))
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
    Heading(t("settings.visuals"))
    Note(t("settings.visuals_note"))
    VisualLookSettings(nav)
    Toggle(t("settings.auto_styles"), t("settings.auto_styles_note"), auto) {
        auto = it
        prefs.visualsAuto = it
    }
    Column(Modifier.padding(horizontal = 16.dp)) {
        Text(t("settings.timing", "ms" to lead.toInt()))
        Text(
            t("settings.timing_note"),
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
        Text(t("settings.strength", "percent" to (intensity * 100).toInt()))
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
    t(
        "settings.usage",
        "downloads" to songCount(downloads.size),
        "download_size" to sizeText(context, downloads.sumOf { it.bytes }),
        "smart" to songCount(smart.size),
        "smart_size" to sizeText(context, smart.sumOf { it.bytes }),
    )

private fun songCount(n: Int) = t("count.songs", "n" to n)

/** The app's language on this phone: the server's default (or the phone's own), or one picked here. */
@Composable
private fun LanguageSettings(nav: Nav) {
    val context = LocalContext.current
    val prefs = nav.prefs
    val languages = remember { com.newterrastudios.djinn.go.i18n.Strings.available(context) }
    if (languages.size < 2) return
    var chosen by remember { mutableStateOf(prefs.language) }
    val house = prefs.houseLanguage
    val houseName = languages.firstOrNull { it.first == house }?.second
    val options = listOf("" to if (houseName != null) t("language.house", "name" to houseName) else t("language.phone")) + languages
    Heading(t("language.label"))
    options.forEach { (code, name) ->
        val pick = {
            if (chosen != code) {
                chosen = code
                prefs.language = code
                com.newterrastudios.djinn.go.i18n.Strings.load(context)
                // Every screen's text comes again in the new language.
                (context as? android.app.Activity)?.recreate()
            }
        }
        Row(
            Modifier
                .fillMaxWidth()
                .clickable(onClick = pick)
                .padding(horizontal = 16.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            RadioButton(selected = chosen == code, onClick = pick)
            Text(name)
        }
    }
}
