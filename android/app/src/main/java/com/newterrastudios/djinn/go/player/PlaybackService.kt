package com.newterrastudios.djinn.go.player

import android.app.PendingIntent
import android.content.Intent
import android.os.SystemClock
import androidx.core.net.toUri
import androidx.media3.common.AudioAttributes
import androidx.media3.common.C
import androidx.media3.common.MediaItem
import androidx.media3.common.Player
import androidx.media3.common.util.UnstableApi
import androidx.media3.datasource.DataSourceBitmapLoader
import androidx.media3.datasource.ResolvingDataSource
import androidx.media3.datasource.cache.CacheDataSource
import androidx.media3.datasource.okhttp.OkHttpDataSource
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.exoplayer.source.DefaultMediaSourceFactory
import androidx.media3.session.CacheBitmapLoader
import androidx.media3.session.MediaSession
import androidx.media3.session.MediaSessionService
import com.google.common.util.concurrent.Futures
import com.google.common.util.concurrent.ListenableFuture
import com.newterrastudios.djinn.go.DjinnApp
import com.newterrastudios.djinn.go.MainActivity
import com.newterrastudios.djinn.go.data.PlayLinks
import com.newterrastudios.djinn.go.data.ResumeSave
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.guava.future
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import java.util.concurrent.ConcurrentHashMap

/** How the player names a song; it becomes a real link only when it's fetched. */
private const val TRACK_SCHEME = "djinn"
private const val SAVE_EVERY_MS = 15_000L
/** Links work for a few hours on the server; a fresh one well before that. */
private const val LINK_LIFE_MS = 60 * 60 * 1000L

private class HeldLink(val links: PlayLinks, val at: Long)

/**
 * Plays songs in the background, with lock screen and headphone controls, and
 * keeps GO told what was played and where it stopped.
 *
 * Fetching goes cache first, so a song played before needs neither a link nor
 * the network. Its cache key is the song, not the link, since links change.
 */
@UnstableApi
class PlaybackService : MediaSessionService() {
    private val app get() = application as DjinnApp
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main)
    /** Telling the server what happened outlives the service. */
    private val reports = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private var reporter: Reporter? = null
    private val links = ConcurrentHashMap<Long, HeldLink>()
    private var session: MediaSession? = null
    private var ticking: Job? = null

    /** The song being heard, and how much of it, for the play report. */
    private var hearing: Long? = null
    private var heardMs = 0L
    private var lastTick = 0L
    private var lastSave = 0L

    override fun onCreate() {
        super.onCreate()
        val network = OkHttpDataSource.Factory(app.api.http)
        val resolving = ResolvingDataSource.Factory(network) { spec ->
            if (spec.uri.scheme != TRACK_SCHEME) return@Factory spec
            val id = spec.uri.lastPathSegment?.toLongOrNull() ?: return@Factory spec
            spec.withUri((app.account.server + link(id).url).toUri())
        }
        val caching = CacheDataSource.Factory()
            .setCache(app.recentCache)
            .setUpstreamDataSourceFactory(resolving)
            .setFlags(CacheDataSource.FLAG_IGNORE_CACHE_ON_ERROR)
        val player = ExoPlayer.Builder(this)
            .setMediaSourceFactory(DefaultMediaSourceFactory(this).setDataSourceFactory(caching))
            .setAudioAttributes(
                AudioAttributes.Builder()
                    .setUsage(C.USAGE_MEDIA)
                    .setContentType(C.AUDIO_CONTENT_TYPE_MUSIC)
                    .build(),
                true,
            )
            .setHandleAudioBecomingNoisy(true)
            .setWakeMode(C.WAKE_MODE_NETWORK)
            .build()
        reporter = Reporter(player).also(player::addListener)
        val open = PendingIntent.getActivity(
            this,
            0,
            Intent(this, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP),
            PendingIntent.FLAG_IMMUTABLE,
        )
        session = MediaSession.Builder(this, player)
            .setSessionActivity(open)
            .setCallback(Callback())
            .setBitmapLoader(
                CacheBitmapLoader(
                    DataSourceBitmapLoader.Builder(this).setDataSourceFactory(network).build(),
                ),
            )
            .build()
    }

    override fun onGetSession(controllerInfo: MediaSession.ControllerInfo) = session

    /** Swiping the app away leaves music playing; with nothing playing, it ends. */
    override fun onTaskRemoved(rootIntent: Intent?) {
        val player = session?.player
        if (player == null || !player.playWhenReady || player.mediaItemCount == 0) stopSelf()
    }

    override fun onDestroy() {
        reporter?.finish()
        session?.run {
            player.release()
            release()
        }
        session = null
        scope.cancel()
        super.onDestroy()
    }

    /** Blocks: only from the player's loading thread. */
    private fun link(id: Long): PlayLinks {
        val held = links[id]
        val now = SystemClock.elapsedRealtime()
        if (held != null && now - held.at < LINK_LIFE_MS) return held.links
        val fresh = app.api.playLinksBlocking(id)
        links[id] = HeldLink(fresh, now)
        scope.launch { session?.player?.let { applyGain(it) } }
        return fresh
    }

    /** Even loudness from the server's ReplayGain. A device can only turn down. */
    private fun applyGain(player: Player) {
        val id = player.currentMediaItem?.mediaId?.toLongOrNull() ?: return
        val held = links[id]
        if (held != null) {
            player.volume = held.links.gain.coerceIn(0f, 1f)
            return
        }
        // Played from the cache, so nothing has asked the server yet.
        scope.launch {
            val fresh = runCatching { app.api.playLinks(id) }.getOrNull() ?: return@launch
            links[id] = HeldLink(fresh, SystemClock.elapsedRealtime())
            if (player.currentMediaItem?.mediaId?.toLongOrNull() == id) {
                player.volume = fresh.gain.coerceIn(0f, 1f)
            }
        }
    }

    private inner class Reporter(private val player: Player) : Player.Listener {
        override fun onMediaItemTransition(mediaItem: MediaItem?, reason: Int) {
            report()
            hearing = mediaItem?.mediaId?.toLongOrNull()
            applyGain(player)
            save()
        }

        override fun onPlaybackStateChanged(playbackState: Int) {
            if (playbackState == Player.STATE_ENDED) report()
        }

        /** The service is going away. */
        fun finish() {
            report()
            save()
        }

        override fun onIsPlayingChanged(isPlaying: Boolean) {
            ticking?.cancel()
            if (!isPlaying) {
                count()
                save()
                return
            }
            hearing = player.currentMediaItem?.mediaId?.toLongOrNull()
            lastTick = SystemClock.elapsedRealtime()
            ticking = scope.launch {
                while (isActive) {
                    delay(1000)
                    count()
                    if (SystemClock.elapsedRealtime() - lastSave > SAVE_EVERY_MS) save()
                }
            }
        }

        private fun count() {
            val now = SystemClock.elapsedRealtime()
            if (lastTick != 0L) heardMs += now - lastTick
            lastTick = if (player.isPlaying) now else 0L
        }

        private fun report() {
            count()
            val id = hearing
            val ms = heardMs
            heardMs = 0
            lastTick = if (player.isPlaying) SystemClock.elapsedRealtime() else 0L
            if (id == null || ms <= 0) return
            reports.launch { runCatching { app.api.reportPlay(id, ms) } }
        }

        private fun save() {
            lastSave = SystemClock.elapsedRealtime()
            val queue = (0 until player.mediaItemCount).mapNotNull {
                player.getMediaItemAt(it).mediaId.toLongOrNull()
            }
            if (queue.isEmpty()) return
            val save = ResumeSave(queue, player.currentMediaItemIndex, player.currentPosition)
            reports.launch { runCatching { app.api.saveResume(save) } }
        }
    }

    private inner class Callback : MediaSession.Callback {
        /** Controllers only send which song; here it gets how to fetch it. */
        override fun onAddMediaItems(
            mediaSession: MediaSession,
            controller: MediaSession.ControllerInfo,
            mediaItems: MutableList<MediaItem>,
        ): ListenableFuture<MutableList<MediaItem>> =
            Futures.immediateFuture(mediaItems.map { playable(it) }.toMutableList())

        /** Headphones or the car asking to play with nothing loaded: where GO left off. */
        override fun onPlaybackResumption(
            mediaSession: MediaSession,
            controller: MediaSession.ControllerInfo,
            isForPlayback: Boolean,
        ): ListenableFuture<MediaSession.MediaItemsWithStartPosition> = scope.future {
            val resume = app.api.resume() ?: throw UnsupportedOperationException("Nothing to resume")
            MediaSession.MediaItemsWithStartPosition(
                resume.songs.map { playable(it.toMediaItem(app.api)) },
                resume.position.coerceIn(0, resume.songs.lastIndex),
                resume.positionMs,
            )
        }
    }

    private fun playable(item: MediaItem): MediaItem = item.buildUpon()
        .setUri("$TRACK_SCHEME://track/${item.mediaId}".toUri())
        // Per server: the same id on another server is another song.
        .setCustomCacheKey("${app.account.server}|${item.mediaId}")
        .build()
}
