package com.newterrastudios.djinn.go.ui

import androidx.annotation.OptIn
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.FilterChip
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.media3.common.util.UnstableApi
import com.newterrastudios.djinn.go.data.GoYear
import com.newterrastudios.djinn.go.i18n.t
import java.io.IOException
import java.text.DateFormatSymbols

/** Your year on DJINN GO: time listened, top songs and artists, busiest month, first song. */
@OptIn(UnstableApi::class)
@Composable
fun YearScreen(nav: Nav) {
    var year by remember { mutableStateOf<Int?>(null) }
    var review by remember { mutableStateOf<GoYear?>(null) }
    var failed by remember { mutableStateOf(false) }
    LaunchedEffect(year) {
        failed = false
        review = try {
            nav.api.year(year)
        } catch (_: IOException) {
            failed = true
            null
        }
    }
    val r = review
    LazyColumn(Modifier.fillMaxSize()) {
        if (r == null) {
            item { Note(if (failed) t("common.no_server") else t("common.loading")) }
            return@LazyColumn
        }
        item {
            Column(Modifier.padding(16.dp)) {
                Text(t("year.title", "year" to r.year), style = MaterialTheme.typography.headlineSmall)
                if (r.years.size > 1) {
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp), modifier = Modifier.padding(top = 8.dp)) {
                        r.years.forEach { y ->
                            FilterChip(selected = y == r.year, onClick = { year = y }, label = { Text("$y") })
                        }
                    }
                }
            }
        }
        if (r.plays == 0L) {
            item { Note(t("year.nothing", "year" to r.year)) }
            return@LazyColumn
        }
        item {
            val listened = if (r.minutes >= 120) t("year.hours", "n" to r.minutes / 60) else t("year.minutes", "n" to r.minutes)
            Column(Modifier.padding(horizontal = 16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Tile(listened, t("year.listened"), Modifier.weight(1f))
                    Tile("${r.plays}", t("year.plays"), Modifier.weight(1f))
                }
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Tile("${r.songs}", t("year.songs"), Modifier.weight(1f))
                    Tile("${r.requests}", t("year.requests"), Modifier.weight(1f))
                }
            }
        }
        if (r.topSongs.isNotEmpty()) {
            item { Heading(t("year.top_songs")) }
            itemsIndexed(r.topSongs) { i, song ->
                Row2(
                    title = "${i + 1}. ${song.title}",
                    subtitle = listOfNotNull(song.artist, t("count.plays", "n" to song.plays)).joinToString(" · "),
                    art = {},
                    onClick = { song.albumId?.let { nav.push(Screen.Album(it)) } },
                )
            }
        }
        if (r.topArtists.isNotEmpty()) {
            item { Heading(t("year.top_artists")) }
            itemsIndexed(r.topArtists) { i, artist ->
                Row2(
                    title = "${i + 1}. ${artist.name}",
                    subtitle = t("count.plays", "n" to artist.plays),
                    art = {},
                    onClick = { nav.push(Screen.Artist(artist.artistId)) },
                )
            }
        }
        r.busiestMonth?.let { busiest ->
            item {
                val names = DateFormatSymbols.getInstance().months
                Heading(t("year.busiest", "month" to names.getOrElse(busiest - 1) { "" }))
                MonthBars(r.months, Modifier.padding(horizontal = 16.dp))
            }
        }
        r.first?.let { song ->
            item { Heading(t("year.first_song", "year" to r.year)) }
            item {
                Row2(
                    title = song.title,
                    subtitle = song.artist,
                    art = { Art(nav.api.artUrl(song.artHash), 56.dp) },
                    onClick = { nav.player.play(listOf(song), 0) },
                )
            }
        }
    }
}

@Composable
private fun Tile(value: String, label: String, modifier: Modifier = Modifier) {
    Column(
        modifier
            .background(MaterialTheme.colorScheme.surfaceVariant, RoundedCornerShape(12.dp))
            .padding(12.dp),
    ) {
        Text(value, style = MaterialTheme.typography.titleLarge)
        Text(label, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
}

/** Plays each month, January first, as bars. */
@Composable
private fun MonthBars(months: List<Long>, modifier: Modifier = Modifier) {
    val max = (months.maxOrNull() ?: 0L).coerceAtLeast(1L)
    val names = DateFormatSymbols.getInstance().shortMonths
    Column(modifier.fillMaxWidth()) {
        Row(
            Modifier
                .fillMaxWidth()
                .height(110.dp),
            horizontalArrangement = Arrangement.spacedBy(2.dp),
            verticalAlignment = Alignment.Bottom,
        ) {
            months.forEachIndexed { i, plays ->
                Box(
                    Modifier
                        .weight(1f)
                        .fillMaxHeight(),
                    contentAlignment = Alignment.BottomCenter,
                ) {
                    Box(
                        Modifier
                            .fillMaxWidth()
                            .fillMaxHeight((plays.toFloat() / max).coerceAtLeast(0.01f))
                            .background(MaterialTheme.colorScheme.primary, RoundedCornerShape(topStart = 4.dp, topEnd = 4.dp))
                            .semantics { contentDescription = "${names.getOrElse(i) { "" }}: ${t("count.plays", "n" to plays)}" },
                    )
                }
            }
        }
        Row(Modifier.fillMaxWidth()) {
            names.take(12).forEach { name ->
                Text(
                    name.take(1),
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.weight(1f),
                    textAlign = androidx.compose.ui.text.style.TextAlign.Center,
                )
            }
        }
    }
}
