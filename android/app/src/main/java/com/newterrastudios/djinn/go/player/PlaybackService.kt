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
import androidx.media3.datasource.DefaultDataSource
import androidx.media3.datasource.ResolvingDataSource
import androidx.media3.datasource.cache.CacheDataSource
import androidx.media3.datasource.okhttp.OkHttpDataSource
import androidx.media3.exoplayer.DefaultRenderersFactory
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.exoplayer.audio.AudioSink
import androidx.media3.exoplayer.audio.DefaultAudioSink
import androidx.media3.exoplayer.audio.TeeAudioProcessor
import androidx.media3.exoplayer.source.DefaultMediaSourceFactory
import androidx.media3.session.CacheBitmapLoader
import androidx.media3.session.LibraryResult
import androidx.media3.session.MediaLibraryService
import androidx.media3.session.MediaSession
import com.google.common.collect.ImmutableList
import com.google.common.util.concurrent.Futures
import com.google.common.util.concurrent.ListenableFuture
import com.newterrastudios.djinn.go.DjinnApp
import com.newterrastudios.djinn.go.MainActivity
import com.newterrastudios.djinn.go.data.PlayLinks
import com.newterrastudios.djinn.go.data.ResumeSave
import com.newterrastudios.djinn.go.data.PendingPlay
import com.newterrastudios.djinn.go.data.Track
import com.newterrastudios.djinn.go.work.SyncWorker
import java.io.File
import java.io.IOException
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
class PlaybackService : MediaLibraryService() {
    private val app get() = application as DjinnApp
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main)
    /** Telling the server what happened outlives the service. */
    private val reports = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private var reporter: Reporter? = null
    private val links = ConcurrentHashMap<Long, HeldLink>()
    private var session: MediaLibrarySession? = null
    private val searches = ConcurrentHashMap<String, List<Track>>()
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
        // Kept songs are plain files; everything else goes through the cache.
        val sources = DefaultDataSource.Factory(this, caching)
        val renderers = object : DefaultRenderersFactory(this) {
            override fun buildAudioSink(
                context: android.content.Context,
                enableFloatOutput: Boolean,
                enableAudioOutputPlaybackParameters: Boolean,
            ): AudioSink = DefaultAudioSink.Builder(context)
                .setEnableFloatOutput(enableFloatOutput)
                .setEnableAudioOutputPlaybackParameters(enableAudioOutputPlaybackParameters)
                .setAudioProcessors(arrayOf(TeeAudioProcessor(Spectrum)))
                .build()
        }
        val player = ExoPlayer.Builder(this, renderers)
            .setMediaSourceFactory(DefaultMediaSourceFactory(this).setDataSourceFactory(sources))
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
        session = MediaLibrarySession.Builder(this, player, Callback())
            .setSessionActivity(open)
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
        // Played from a kept copy or the cache, so nothing has asked the server yet.
        scope.launch {
            val kept = app.offline.local(id)
            if (kept != null) {
                if (player.currentMediaItem?.mediaId?.toLongOrNull() == id) {
                    player.volume = kept.gain.coerceIn(0f, 1f)
                }
                return@launch
            }
            val fresh = runCatching { app.api.playLinks(id) }.getOrNull() ?: return@launch
            links[id] = HeldLink(fresh, SystemClock.elapsedRealtime())
            if (player.currentMediaItem?.mediaId?.toLongOrNull() == id) {
                player.volume = fresh.gain.coerceIn(0f, 1f)
            }
        }
    }

    private inner class Reporter(private val player: Player) : Player.Listener {
        override fun onEvents(player: Player, events: Player.Events) {
            val item = player.currentMediaItem?.mediaMetadata
            NowPlayingWidget.show(
                this@PlaybackService,
                item?.title?.toString(),
                item?.artist?.toString(),
                player.isPlaying,
            )
        }

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
            val at = System.currentTimeMillis()
            val server = app.account.server
            reports.launch {
                try {
                    app.api.reportPlay(id, ms, at)
                } catch (e: IOException) {
                    // Not reachable: it goes when it can. The server saying no is final.
                    if (e !is com.newterrastudios.djinn.go.data.ApiException && server != null) {
                        app.offline.dao.addPlay(PendingPlay(server = server, trackId = id, msPlayed = ms, at = at))
                        SyncWorker.enqueue(this@PlaybackService)
                    }
                }
            }
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

    private inner class Callback : MediaLibrarySession.Callback {
        /**
         * Controllers only send which song; here it gets how to fetch it. A
         * voice request sends words instead, and plays what they find. The car
         * sends just an id, so the rest is looked up.
         */
        override fun onAddMediaItems(
            mediaSession: MediaSession,
            controller: MediaSession.ControllerInfo,
            mediaItems: MutableList<MediaItem>,
        ): ListenableFuture<MutableList<MediaItem>> = scope.future {
            mediaItems.flatMap { item ->
                val query = item.requestMetadata.searchQuery
                when {
                    query != null -> spoken(query).map { playable(it.toMediaItem(app.api)) }
                    item.mediaMetadata.title == null -> {
                        val id = item.mediaId.toLongOrNull()
                        val track = id?.let { runCatching { app.api.track(it) }.getOrNull() }
                            ?: id?.let { app.offline.local(it)?.toTrack() }
                        listOfNotNull(track?.let { playable(it.toMediaItem(app.api)) })
                    }
                    else -> listOf(playable(item))
                }
            }.toMutableList()
        }

        // What Android Auto browses: lately played, downloads, playlists, albums.
        override fun onGetLibraryRoot(
            session: MediaLibrarySession,
            browser: MediaSession.ControllerInfo,
            params: LibraryParams?,
        ) = Futures.immediateFuture(LibraryResult.ofItem(folder(ROOT, "DJINN GO"), params))

        override fun onGetChildren(
            session: MediaLibrarySession,
            browser: MediaSession.ControllerInfo,
            parentId: String,
            page: Int,
            pageSize: Int,
            params: LibraryParams?,
        ) = scope.future<LibraryResult<ImmutableList<MediaItem>>> {
            LibraryResult.ofItemList(ImmutableList.copyOf(children(parentId)), params)
        }

        override fun onGetItem(
            session: MediaLibrarySession,
            browser: MediaSession.ControllerInfo,
            mediaId: String,
        ) = scope.future<LibraryResult<MediaItem>> {
            val id = mediaId.toLongOrNull()
            val track = id?.let { runCatching { app.api.track(it) }.getOrNull() }
            if (track != null) {
                LibraryResult.ofItem(track.toMediaItem(app.api), null)
            } else {
                LibraryResult.ofError(LibraryResult.RESULT_ERROR_BAD_VALUE)
            }
        }

        override fun onSearch(
            session: MediaLibrarySession,
            browser: MediaSession.ControllerInfo,
            query: String,
            params: LibraryParams?,
        ) = scope.future<LibraryResult<Void>> {
            val found = spoken(query)
            searches[query] = found
            session.notifySearchResultChanged(browser, query, found.size, params)
            LibraryResult.ofVoid()
        }

        override fun onGetSearchResult(
            session: MediaLibrarySession,
            browser: MediaSession.ControllerInfo,
            query: String,
            page: Int,
            pageSize: Int,
            params: LibraryParams?,
        ) = scope.future<LibraryResult<ImmutableList<MediaItem>>> {
            val found = searches[query] ?: spoken(query)
            LibraryResult.ofItemList(ImmutableList.copyOf(found.map { it.toMediaItem(app.api) }), params)
        }

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

    /** Songs for spoken or typed words, or from what's kept without a server. */
    private suspend fun spoken(query: String): List<Track> {
        val server = app.account.server ?: return emptyList()
        return try {
            val found = app.api.search(query)
            found.tracks.ifEmpty { found.described?.tracks.orEmpty() }
        } catch (_: IOException) {
            app.offline.dao.search(server, "%$query%").map { it.toTrack() }
        }
    }

    private fun folder(id: String, title: String): MediaItem = MediaItem.Builder()
        .setMediaId(id)
        .setMediaMetadata(
            androidx.media3.common.MediaMetadata.Builder()
                .setTitle(title)
                .setIsBrowsable(true)
                .setIsPlayable(false)
                .build(),
        )
        .build()

    private suspend fun children(parent: String): List<MediaItem> {
        val api = app.api
        val server = app.account.server ?: return emptyList()
        return try {
            when {
                parent == ROOT -> listOf(
                    folder(RECENT, "Lately played"),
                    folder(KEPT, "On this phone"),
                    folder(PLAYLISTS, "Playlists"),
                    folder(ALBUMS, "Albums"),
                )
                parent == RECENT -> api.home().recent.map { it.toMediaItem(api) }
                parent == KEPT -> app.offline.dao.songs(server).map { it.toTrack().toMediaItem(api) }
                parent == PLAYLISTS -> api.playlists().map { folder("$PLAYLIST_PREFIX${it.id}", it.name) }
                parent == ALBUMS -> api.albums(0).items.map { folder("$ALBUM_PREFIX${it.id}", it.title) }
                parent.startsWith(PLAYLIST_PREFIX) ->
                    api.playlist(parent.removePrefix(PLAYLIST_PREFIX).toLong()).songs.map { it.toMediaItem(api) }
                parent.startsWith(ALBUM_PREFIX) ->
                    api.album(parent.removePrefix(ALBUM_PREFIX).toLong()).tracks.map { it.toMediaItem(api) }
                else -> emptyList()
            }
        } catch (_: IOException) {
            // Away from the server, the car still has what's kept here.
            when (parent) {
                ROOT -> listOf(folder(KEPT, "On this phone"))
                KEPT -> app.offline.dao.songs(server).map { it.toTrack().toMediaItem(api) }
                else -> emptyList()
            }
        }
    }

    /** A kept copy plays from its file; anything else as a song to fetch. */
    private suspend fun playable(item: MediaItem): MediaItem {
        val id = item.mediaId.toLongOrNull()
        val kept = id?.let { app.offline.local(it) }
        if (kept != null) {
            return item.buildUpon().setUri(android.net.Uri.fromFile(File(kept.path))).build()
        }
        return item.buildUpon()
            .setUri("$TRACK_SCHEME://track/${item.mediaId}".toUri())
            // Per server: the same id on another server is another song.
            .setCustomCacheKey("${app.account.server}|${item.mediaId}")
            .build()
    }

    private companion object {
        const val ROOT = "root"
        const val RECENT = "recent"
        const val KEPT = "kept"
        const val PLAYLISTS = "playlists"
        const val ALBUMS = "albums"
        const val PLAYLIST_PREFIX = "playlist-"
        const val ALBUM_PREFIX = "album-"
    }
}
