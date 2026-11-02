package com.newterrastudios.djinn.go.player

import android.content.ComponentName
import android.content.Context
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
import kotlinx.coroutines.delay
import kotlinx.coroutines.guava.await

fun Track.toMediaItem(api: GoApi): MediaItem = MediaItem.Builder()
    .setMediaId(id.toString())
    .setMediaMetadata(
        MediaMetadata.Builder()
            .setTitle(title)
            .setArtist(artist)
            .setAlbumTitle(album)
            .setDurationMs(durationMs)
            .setArtworkUri(api.artUrl(artHash, 512)?.toUri())
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
        }

        override fun onPlayerError(e: PlaybackException) {
            error = e.cause?.message?.takeIf { "429" in it }
                ?.let { "You're already listening on as many devices as your role allows." }
                ?: "Couldn't play this song (${e.errorCodeName})."
        }
    }

    suspend fun connect(context: Context) {
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
        player.setMediaItems(songs.map { it.toMediaItem(api) }, start, 0)
        player.prepare()
        player.play()
    }

    fun resume(saved: GoResume) {
        val player = controller ?: return
        error = null
        addedAhead = 0
        player.setMediaItems(
            saved.songs.map { it.toMediaItem(api) },
            saved.position.coerceIn(0, saved.songs.lastIndex),
            saved.positionMs,
        )
        player.prepare()
        player.play()
    }

    /** With nothing playing this starts it, so a tap always does something. */
    fun playNext(track: Track) = insert(track, front = true)

    fun addToQueue(track: Track) = insert(track, front = false)

    private fun insert(track: Track, front: Boolean) {
        val player = controller ?: return
        if (player.mediaItemCount == 0) return play(listOf(track))
        val next = player.currentMediaItemIndex + 1
        val at = if (front) next else next + addedAhead
        player.addMediaItem(at.coerceAtMost(player.mediaItemCount), track.toMediaItem(api))
        addedAhead++
    }

    /** Lines radio up after the current song and anything you added. */
    fun queueRadio(songs: List<Track>) {
        val player = controller ?: return
        if (player.mediaItemCount == 0) return play(songs)
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

    fun jumpTo(index: Int) {
        controller?.seekToDefaultPosition(index)
    }

    fun remove(index: Int) {
        controller?.removeMediaItem(index)
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
