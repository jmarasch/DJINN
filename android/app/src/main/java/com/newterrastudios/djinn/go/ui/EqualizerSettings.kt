package com.newterrastudios.djinn.go.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.material3.FilterChip
import androidx.compose.material3.MaterialTheme
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
import androidx.compose.ui.unit.dp
import androidx.media3.common.util.UnstableApi
import com.newterrastudios.djinn.go.i18n.t
import com.newterrastudios.djinn.go.player.EQ_BAND_HZ
import com.newterrastudios.djinn.go.player.EQ_MAX_BAND_DB
import com.newterrastudios.djinn.go.player.EQ_MAX_OUTPUT_DB
import com.newterrastudios.djinn.go.player.EQ_MIN_OUTPUT_DB
import com.newterrastudios.djinn.go.player.EQ_PRESETS
import com.newterrastudios.djinn.go.player.EqSettings
import com.newterrastudios.djinn.go.player.GoEqualizer
import java.util.Locale

/** "32", "1k", "16k" for a band. */
private fun bandLabel(hz: Int) = if (hz >= 1000) "${hz / 1000}k" else "$hz"

private fun dbLabel(db: Float) = String.format(Locale.ROOT, "%+.1f dB", db)

/** Your own equalizer, in Settings: on or off, presets, twelve bands and the output level. */
@UnstableApi
@OptIn(ExperimentalLayoutApi::class)
@Composable
internal fun EqualizerSettings(nav: Nav) {
    var settings by remember { mutableStateOf(nav.prefs.equalizer) }
    fun change(next: EqSettings) {
        settings = next
        GoEqualizer.settings = next
        nav.prefs.equalizer = next
    }
    Heading(t("eq.title"))
    Note(t("eq.phone_note"))
    Toggle(t("eq.on"), if (settings.enabled) t("eq.applies") else t("eq.turn_on"), settings.enabled) {
        change(settings.copy(enabled = it))
    }
    if (!settings.enabled) return
    val preset = EQ_PRESETS.firstOrNull { it.second == settings.bandsDb }?.first
    FlowRow(
        Modifier.padding(horizontal = 16.dp),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        EQ_PRESETS.forEach { (key, gains) ->
            FilterChip(
                selected = preset == key,
                onClick = { change(settings.copy(bandsDb = gains)) },
                label = { Text(t(key)) },
            )
        }
        if (preset == null) {
            Text(
                t("eq.custom"),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.align(Alignment.CenterVertically),
            )
        }
    }
    Column(Modifier.padding(horizontal = 16.dp)) {
        EQ_BAND_HZ.forEachIndexed { i, hz ->
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(bandLabel(hz), style = MaterialTheme.typography.bodySmall, modifier = Modifier.width(36.dp))
                Slider(
                    value = settings.bandsDb[i],
                    onValueChange = { db ->
                        val rounded = Math.round(db * 2) / 2f
                        change(settings.copy(bandsDb = settings.bandsDb.toMutableList().also { it[i] = rounded }))
                    },
                    valueRange = -EQ_MAX_BAND_DB..EQ_MAX_BAND_DB,
                    modifier = Modifier.weight(1f),
                )
                Text(
                    dbLabel(settings.bandsDb[i]),
                    style = MaterialTheme.typography.bodySmall,
                    modifier = Modifier.width(64.dp).padding(start = 8.dp),
                )
            }
        }
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(t("eq.output_level"), modifier = Modifier.weight(1f))
            Text(dbLabel(settings.outputGainDb), style = MaterialTheme.typography.bodySmall)
        }
        Slider(
            value = settings.outputGainDb,
            onValueChange = { change(settings.copy(outputGainDb = Math.round(it * 2) / 2f)) },
            valueRange = EQ_MIN_OUTPUT_DB..EQ_MAX_OUTPUT_DB,
            modifier = Modifier.fillMaxWidth(),
        )
        TextButton(
            enabled = preset != "eq.flat" || settings.outputGainDb != 0f,
            onClick = { change(settings.copy(bandsDb = EQ_PRESETS.first().second, outputGainDb = 0f)) },
        ) { Text(t("eq.reset")) }
    }
}
