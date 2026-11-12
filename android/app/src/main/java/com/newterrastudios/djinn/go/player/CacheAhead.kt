package com.newterrastudios.djinn.go.player

import androidx.media3.common.C
import androidx.media3.common.Player
import androidx.media3.common.util.UnstableApi
import androidx.media3.datasource.DataSpec
import androidx.media3.datasource.cache.CacheDataSource
import androidx.media3.datasource.cache.CacheWriter
import androidx.core.net.toUri
import java.io.IOException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/** Waits this long after a change, so the song starting gets the connection first. */
private const val SETTLE_MS = 3_000L

/**
 * Where the next [count] songs are, after [current], in the order they play:
 * [next] is the timeline's next index (C.INDEX_UNSET at the end). Stops
 * when the order comes round to a song already listed.
 */
fun aheadIndices(current: Int, count: Int, next: (Int) -> Int): List<Int> {
    val found = mutableListOf<Int>()
    var index = current
    while (found.size < count) {
        index = next(index)
        if (index == C.INDEX_UNSET || index == current || index in found) break
        found += index
    }
    return found
}

/**
 * Fetches the next few songs in the queue into the recent cache, one at a
 * time, so they play through a weak signal or a lost one. Songs already
 * cached or kept on the phone cost nothing; a change to the queue starts
 * over from the song after the one playing.
 */
@UnstableApi
class CacheAhead(
    private val scope: CoroutineScope,
    private val source: CacheDataSource.Factory,
    /** How many songs to fetch ahead; 0 is off. */
    private val howMany: () -> Int,
    /** The cache key for a song, or null when it plays from a file instead. */
    private val keyFor: suspend (String) -> String?,
) : Player.Listener {
    private var job: Job? = null
    @Volatile private var writer: CacheWriter? = null

    override fun onEvents(player: Player, events: Player.Events) {
        if (events.containsAny(
                Player.EVENT_MEDIA_ITEM_TRANSITION,
                Player.EVENT_TIMELINE_CHANGED,
                Player.EVENT_SHUFFLE_MODE_ENABLED_CHANGED,
                Player.EVENT_REPEAT_MODE_CHANGED,
            )
        ) {
            restart(player)
        }
    }

    fun stop() {
        job?.cancel()
        writer?.cancel()
    }

    private fun restart(player: Player) {
        stop()
        val count = howMany()
        if (count <= 0 || player.mediaItemCount == 0) return
        val timeline = player.currentTimeline
        // Repeating one song still moves on to the next when it's skipped.
        val repeat = if (player.repeatMode == Player.REPEAT_MODE_ONE) Player.REPEAT_MODE_OFF else player.repeatMode
        val shuffle = player.shuffleModeEnabled
        val ids = aheadIndices(player.currentMediaItemIndex, count) {
            timeline.getNextWindowIndex(it, repeat, shuffle)
        }.map { player.getMediaItemAt(it).mediaId }
        job = scope.launch {
            delay(SETTLE_MS)
            for (id in ids) {
                if (!isActive) break
                val key = keyFor(id) ?: continue
                withContext(Dispatchers.IO) { fetch(id, key) }
            }
        }
    }

    /** Blocks until the song is in the cache, or fails, or is called off. */
    private fun fetch(id: String, key: String) {
        val spec = DataSpec.Builder()
            .setUri("$TRACK_SCHEME://track/$id".toUri())
            .setKey(key)
            .build()
        val cacheWriter = CacheWriter(source.createDataSource(), spec, null, null)
        writer = cacheWriter
        try {
            cacheWriter.cache()
        } catch (_: IOException) {
            // Offline or refused: the player tries again itself when it gets there.
        } catch (_: RuntimeException) {
            // A link the server wouldn't give, for one.
        } finally {
            if (writer === cacheWriter) writer = null
        }
    }
}
