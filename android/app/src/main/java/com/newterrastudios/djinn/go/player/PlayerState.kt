package com.newterrastudios.djinn.go.player

import com.newterrastudios.djinn.go.data.OfflineSong
import com.newterrastudios.djinn.go.data.keptMediaId
import android.content.ComponentName
import android.content.Context
import android.os.Bundle
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.core.net.toUri
import androidx.media3.common.MediaItem
import androidx.media3.common.MediaMetadata
import androidx.media3.common.PlaybackException
import androidx.media3.common.Player
import androidx.media3.common.util.UnstableApi
import androidx.media3.session.MediaController
import androidx.media3.session.SessionToken
import com.newterrastudios.djinn.go.data.GoApi
import com.newterrastudios.djinn.go.data.GoResume
import com.newterrastudios.djinn.go.data.Track
import com.newterrastudios.djinn.go.i18n.t
import java.io.IOException
import kotlinx.coroutines.MainScope
import kotlinx.coroutines.delay
import kotlinx.coroutines.guava.await
import kotlinx.coroutines.launch

/** Songs left when a radio asks for more. */
private const val RADIO_LOW = 5
private const val RADIO_EXCLUDE = 400

private const val ARTIST_ID = "com.newterrastudios.djinn.go.artist_id"
private const val ALBUM_ID = "com.newterrastudios.djinn.go.album_id"

/** The song's artist on the server in use, if it can be opened there. */
val MediaItem.artistId: Long? get() = libraryId(ARTIST_ID)

/** The song's album on the server in use, if it can be opened there. */
val MediaItem.albumId: Long? get() = libraryId(ALBUM_ID)

private fun MediaItem.libraryId(key: String): Long? =
    mediaMetadata.extras?.takeIf { it.containsKey(key) }?.getLong(key)

fun Track.toMediaItem(api: GoApi): MediaItem = MediaItem.Builder()
    .setMediaId(id.toString())
    .setMediaMetadata(
        MediaMetadata.Builder()
            .setTitle(title)
            .setArtist(artist)
            .setAlbumTitle(album)
            .setDurationMs(durationMs)
            .setArtworkUri(api.artUrl(artHash, 512)?.toUri())
            .setExtras(
                Bundle().apply {
                    artistId?.let { putLong(ARTIST_ID, it) }
                    albumId?.let { putLong(ALBUM_ID, it) }
                },
            )
            // A library list is refused without these.
            .setIsBrowsable(false)
            .setIsPlayable(true)
            .build(),
    )
    .build()

/**
 * A kept song from another server: no picture, since that server may be out of
 * reach, and no artist or album to open, since those ids are that server's.
 */
fun OfflineSong.toKeptItem(): MediaItem = MediaItem.Builder()
    .setMediaId(keptMediaId(server, id))
    .setMediaMetadata(
        MediaMetadata.Builder()
            .setTitle(title)
            .setArtist(artist)
            .setAlbumTitle(album)
            .setDurationMs(durationMs)
            .setIsBrowsable(false)
            .setIsPlayable(true)
            .build(),
    )
    .build()

/** One entry in Up next. */
data class QueueEntry(val index: Int, val item: MediaItem)

/** The player as the screens see it, from the service that really plays. */
@UnstableApi
class PlayerState(private val api: GoApi) {
    private var controller: MediaController? = null

    var current by mutableStateOf<MediaItem?>(null)
        private set
    var playing by mutableStateOf(false)
        private set
    var loading by mutableStateOf(false)
        private set
    var positionMs by mutableLongStateOf(0L)
        private set
    var durationMs by mutableLongStateOf(0L)
        private set
    var shuffle by mutableStateOf(false)
        private set
    var repeat by mutableIntStateOf(Player.REPEAT_MODE_OFF)
        private set
    var upNext by mutableStateOf<List<QueueEntry>>(emptyList())
        private set
    var hasPrevious by mutableStateOf(false)
        private set
    var error by mutableStateOf<String?>(null)
    var volume by mutableFloatStateOf(1f)
        private set

    /** Songs added by hand, which play ahead of the rest of the queue. */
    private var addedAhead = 0

    private val listener = object : Player.Listener {
        override fun onEvents(player: Player, events: Player.Events) = refresh(player)

        override fun onMediaItemTransition(mediaItem: MediaItem?, reason: Int) {
            if (reason == Player.MEDIA_ITEM_TRANSITION_REASON_AUTO ||
                reason == Player.MEDIA_ITEM_TRANSITION_REASON_SEEK
            ) {
                if (addedAhead > 0) addedAhead--
            }
            extendRadio()
        }

        override fun onPlayerError(e: PlaybackException) {
            error = e.cause?.message?.takeIf { "429" in it }
                ?.let { t("player.too_many_streams") }
                ?: t("player.play_failed", "error" to e.errorCodeName)
        }
    }

    suspend fun connect(context: Context) {
        if (controller != null) return
        val token = SessionToken(context, ComponentName(context, PlaybackService::class.java))
        val connected = MediaController.Builder(context, token).buildAsync().await()
        connected.addListener(listener)
        controller = connected
        refresh(connected)
    }

    fun disconnect() {
        controller?.removeListener(listener)
        controller?.release()
        controller = null
    }

    /** Milliseconds into the song, read now (lyrics need more than [positionMs]'s two updates a second). */
    fun livePositionMs(): Long = controller?.currentPosition?.coerceAtLeast(0) ?: positionMs

    /** Follows the position while something's on screen to show it. */
    suspend fun followPosition() {
        while (true) {
            controller?.let {
                positionMs = it.currentPosition.coerceAtLeast(0)
                durationMs = it.duration.coerceAtLeast(0)
            }
            delay(500)
        }
    }

    private fun refresh(player: Player) {
        current = player.currentMediaItem
        playing = player.isPlaying
        loading = player.playbackState == Player.STATE_BUFFERING
        positionMs = player.currentPosition.coerceAtLeast(0)
        durationMs = player.duration.coerceAtLeast(0)
        shuffle = player.shuffleModeEnabled
        repeat = player.repeatMode
        hasPrevious = player.hasPreviousMediaItem()
        volume = player.volume
        val at = player.currentMediaItemIndex
        upNext = (at + 1 until player.mediaItemCount).map { QueueEntry(it, player.getMediaItemAt(it)) }
    }

    fun play(songs: List<Track>, start: Int = 0) {
        val player = controller ?: return
        error = null
        addedAhead = 0
        radioSeed = null
        player.setMediaItems(songs.map { it.toMediaItem(api) }, start, 0)
        player.prepare()
        player.play()
    }

    /** Turns shuffle on and starts from any of the songs. */
    fun playShuffled(songs: List<Track>) {
        if (songs.isEmpty()) return
        controller?.shuffleModeEnabled = true
        play(songs, songs.indices.random())
    }

    /** Plays kept songs from any server, in order. */
    fun playKept(songs: List<OfflineSong>, start: Int, currentScope: String) {
        val player = controller ?: return
        error = null
        addedAhead = 0
        radioSeed = null
        val items = songs.map { if (it.server == currentScope) it.toTrack().toMediaItem(api) else it.toKeptItem() }
        player.setMediaItems(items, start, 0)
        player.prepare()
        player.play()
    }

    fun resume(saved: GoResume) {
        val player = controller ?: return
        error = null
        addedAhead = 0
        radioSeed = null
        player.setMediaItems(
            saved.songs.map { it.toMediaItem(api) },
            saved.position.coerceIn(0, saved.songs.lastIndex),
            saved.positionMs,
        )
        player.prepare()
        player.play()
    }

    /**
     * After the song playing and what you lined up before, so several play in
     * the order you chose them. With nothing playing this starts it.
     */
    fun playNext(track: Track) {
        val player = controller ?: return
        if (player.mediaItemCount == 0) return play(listOf(track))
        val at = player.currentMediaItemIndex + 1 + addedAhead
        player.addMediaItem(at.coerceAtMost(player.mediaItemCount), track.toMediaItem(api))
        addedAhead++
    }

    /** After everything in the queue. */
    fun addToEnd(track: Track) {
        val player = controller ?: return
        if (player.mediaItemCount == 0) return play(listOf(track))
        player.addMediaItem(track.toMediaItem(api))
    }

    /** Moves a song still to come to another place still to come. */
    fun move(from: Int, to: Int) {
        val player = controller ?: return
        val first = player.currentMediaItemIndex + 1
        if (from < first || from >= player.mediaItemCount) return
        val target = to.coerceIn(first, player.mediaItemCount - 1)
        if (target != from) player.moveMediaItem(from, target)
    }

    /** The song a radio started from, while one is on. */
    private var radioSeed: Long? = null
    private var extending = false
    private val scope = MainScope()

    /** More radio as it runs low, from the song it started with. */
    private fun extendRadio() {
        val player = controller ?: return
        val seed = radioSeed ?: return
        if (extending || player.mediaItemCount - player.currentMediaItemIndex > RADIO_LOW) return
        extending = true
        scope.launch {
            try {
                val known = (0 until player.mediaItemCount).mapNotNull { player.getMediaItemAt(it).mediaId.toLongOrNull() }
                // The newest are what matter if the queue has grown very long.
                val lined = known.takeLast(RADIO_EXCLUDE)
                var more = api.radio(seed, lined).filter { it.id !in known }
                if (more.isEmpty()) {
                    // Nothing left around the seed: wander on from the last song.
                    more = api.radio(known.last(), lined).filter { it.id !in known }
                }
                if (radioSeed == seed) player.addMediaItems(more.map { it.toMediaItem(api) })
            } catch (_: IOException) {
                // Offline: the radio ends with what it has.
            } finally {
                extending = false
            }
        }
    }

    /** Lines radio up after the current song and anything you added. */
    fun queueRadio(seed: Long, songs: List<Track>) {
        val player = controller ?: return
        radioSeed = seed
        if (player.mediaItemCount == 0) {
            play(songs)
            radioSeed = seed
            return
        }
        val at = player.currentMediaItemIndex + 1 + addedAhead
        player.removeMediaItems(at.coerceAtMost(player.mediaItemCount), player.mediaItemCount)
        val playing = player.currentMediaItem?.mediaId
        player.addMediaItems(songs.filter { it.id.toString() != playing }.map { it.toMediaItem(api) })
        // A short song may have ended while the radio was on its way.
        if (player.playbackState == Player.STATE_ENDED && at < player.mediaItemCount) {
            player.seekToDefaultPosition(at)
            player.play()
        }
    }

    fun toggle() {
        val player = controller ?: return
        if (player.isPlaying) {
            player.pause()
        } else {
            error = null
            if (player.playbackState == Player.STATE_IDLE) player.prepare()
            player.play()
        }
    }

    fun next() {
        controller?.seekToNextMediaItem()
    }

    /** To the song's start, or the song before if just begun. */
    fun previous() {
        controller?.seekToPrevious()
    }

    fun seekTo(ms: Long) {
        controller?.seekTo(ms)
        positionMs = ms
    }

    /**
     * Plays a song from Up next. The songs between it and the one playing go,
     * as if skipped through; the one playing stays behind it for Previous.
     */
    fun jumpTo(index: Int) {
        val player = controller ?: return
        val next = player.currentMediaItemIndex + 1
        // Shuffled, the list's order isn't the playing order: just jump.
        if (!player.shuffleModeEnabled && index > next) {
            player.removeMediaItems(next, index)
            player.seekToDefaultPosition(next)
        } else {
            player.seekToDefaultPosition(index)
        }
    }

    fun remove(index: Int) {
        controller?.removeMediaItem(index)
    }

    /** Takes everything after the song playing out of the queue. Gives back how to undo it. */
    fun clearUpcoming(): () -> Unit {
        val player = controller ?: return {}
        val gone = upNext.map { it.item }
        val seed = radioSeed
        val ahead = addedAhead
        // Radio would only fill it up again.
        radioSeed = null
        addedAhead = 0
        player.removeMediaItems(player.currentMediaItemIndex + 1, player.mediaItemCount)
        return {
            controller?.takeIf { it.mediaItemCount > 0 }?.let {
                it.addMediaItems(it.currentMediaItemIndex + 1, gone)
                radioSeed = seed
                addedAhead = ahead
            }
        }
    }

    /** Stops the song playing and empties the queue. Gives back how to undo it. */
    fun clearAll(): () -> Unit {
        val player = controller ?: return {}
        val items = (0 until player.mediaItemCount).map { player.getMediaItemAt(it) }
        val index = player.currentMediaItemIndex
        val position = player.currentPosition
        val seed = radioSeed
        val ahead = addedAhead
        radioSeed = null
        addedAhead = 0
        stop()
        return {
            controller?.takeIf { it.mediaItemCount == 0 && items.isNotEmpty() }?.let {
                it.setMediaItems(items, index, position)
                it.prepare()
                it.play()
                radioSeed = seed
                addedAhead = ahead
            }
        }
    }

    fun toggleShuffle() {
        controller?.let { it.shuffleModeEnabled = !it.shuffleModeEnabled }
    }

    fun cycleRepeat() {
        controller?.let {
            it.repeatMode = when (it.repeatMode) {
                Player.REPEAT_MODE_OFF -> Player.REPEAT_MODE_ALL
                Player.REPEAT_MODE_ALL -> Player.REPEAT_MODE_ONE
                else -> Player.REPEAT_MODE_OFF
            }
        }
    }

    /** What casting takes over: the song playing and what follows. */
    fun queueFromCurrent(): List<MediaItem> {
        val player = controller ?: return emptyList()
        return (player.currentMediaItemIndex until player.mediaItemCount).map { player.getMediaItemAt(it) }
    }

    fun pauseForCast() {
        controller?.pause()
    }

    fun stop() {
        controller?.run {
            stop()
            clearMediaItems()
        }
    }
}
