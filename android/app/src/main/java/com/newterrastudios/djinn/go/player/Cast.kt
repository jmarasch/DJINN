package com.newterrastudios.djinn.go.player

import android.content.Context
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.core.net.toUri
import androidx.media3.common.MediaItem
import androidx.media3.common.util.UnstableApi
import androidx.mediarouter.media.MediaRouteSelector
import androidx.mediarouter.media.MediaRouter
import com.google.android.gms.cast.CastMediaControlIntent
import com.google.android.gms.cast.MediaInfo
import com.google.android.gms.cast.MediaLoadRequestData
import com.google.android.gms.cast.MediaMetadata as CastMetadata
import com.google.android.gms.cast.MediaQueueData
import com.google.android.gms.cast.MediaQueueItem
import com.google.android.gms.cast.framework.CastContext
import com.google.android.gms.cast.framework.CastOptions
import com.google.android.gms.cast.framework.CastSession
import com.google.android.gms.cast.framework.OptionsProvider
import com.google.android.gms.cast.framework.SessionManagerListener
import com.google.android.gms.common.images.WebImage
import com.newterrastudios.djinn.go.DjinnApp
import com.newterrastudios.djinn.go.i18n.t
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import java.io.IOException
import java.util.concurrent.Executors

/** Tells the Cast library to use Google's built-in receiver for plain audio. */
class CastOptionsProvider : OptionsProvider {
    override fun getCastOptions(context: Context): CastOptions = CastOptions.Builder()
        .setReceiverApplicationId(CastMediaControlIntent.DEFAULT_MEDIA_RECEIVER_APPLICATION_ID)
        .build()

    override fun getAdditionalSessionProviders(context: Context) = null
}

/**
 * Sends the queue to a Chromecast or Google TV. The receiver fetches each song
 * itself by its link, so the server's address must work from the receiver too.
 * Phones without Google Play services simply show no devices.
 */
@UnstableApi
class CastState(private val context: Context, private val player: PlayerState) {
    private val app get() = context.applicationContext as DjinnApp
    private val scope = CoroutineScope(Dispatchers.Main)
    private val router by lazy { MediaRouter.getInstance(context) }
    private val selector by lazy {
        MediaRouteSelector.Builder()
            .addControlCategory(CastMediaControlIntent.categoryForCast(CastMediaControlIntent.DEFAULT_MEDIA_RECEIVER_APPLICATION_ID))
            .build()
    }
    private var cast: CastContext? = null

    var devices by mutableStateOf<List<MediaRouter.RouteInfo>>(emptyList())
        private set
    var castingTo by mutableStateOf<String?>(null)
        private set
    var remotePlaying by mutableStateOf(false)
        private set

    private val routes = object : MediaRouter.Callback() {
        override fun onRouteAdded(router: MediaRouter, route: MediaRouter.RouteInfo) = list()
        override fun onRouteRemoved(router: MediaRouter, route: MediaRouter.RouteInfo) = list()
        override fun onRouteChanged(router: MediaRouter, route: MediaRouter.RouteInfo) = list()
    }

    private val sessions = object : SessionManagerListener<CastSession> {
        override fun onSessionStarted(session: CastSession, sessionId: String) = begin(session)
        override fun onSessionResumed(session: CastSession, wasSuspended: Boolean) {
            castingTo = session.castDevice?.friendlyName
        }
        override fun onSessionEnded(session: CastSession, error: Int) = finished()
        override fun onSessionStarting(session: CastSession) = Unit
        override fun onSessionStartFailed(session: CastSession, error: Int) {
            player.error = t("cast.failed")
        }
        override fun onSessionEnding(session: CastSession) = Unit
        override fun onSessionResuming(session: CastSession, sessionId: String) = Unit
        override fun onSessionResumeFailed(session: CastSession, error: Int) = finished()
        override fun onSessionSuspended(session: CastSession, reason: Int) = Unit
    }

    /** Starts looking for devices. Nothing happens without Play services. */
    fun start() {
        if (cast == null) {
            CastContext.getSharedInstance(context, Executors.newSingleThreadExecutor())
                .addOnSuccessListener { shared ->
                    cast = shared
                    shared.sessionManager.addSessionManagerListener(sessions, CastSession::class.java)
                    router.addCallback(selector, routes, MediaRouter.CALLBACK_FLAG_REQUEST_DISCOVERY)
                    list()
                }
                .addOnFailureListener { /* No Play services: no casting. */ }
        } else {
            router.addCallback(selector, routes, MediaRouter.CALLBACK_FLAG_REQUEST_DISCOVERY)
            list()
        }
    }

    fun stopLooking() {
        if (cast != null) router.removeCallback(routes)
    }

    private fun list() {
        devices = router.routes.filter { !it.isDefault && it.matchesSelector(selector) }
    }

    fun castTo(route: MediaRouter.RouteInfo) {
        router.selectRoute(route)
    }

    /** Back to this phone, where it left off. */
    fun stop() {
        cast?.sessionManager?.endCurrentSession(true)
    }

    private fun begin(session: CastSession) {
        castingTo = session.castDevice?.friendlyName
        val items = player.queueFromCurrent()
        if (items.isEmpty()) return
        val position = player.positionMs
        player.pauseForCast()
        scope.launch {
            try {
                // Another server's kept songs only exist here, so there's nothing to send a TV.
                val castable = items.filter { it.mediaId.toLongOrNull() != null }
                if (castable.isEmpty()) {
                    player.error = t("cast.other_server")
                    stop()
                    return@launch
                }
                val queue = castable.map { item -> MediaQueueItem.Builder(info(item)).build() }
                val data = MediaQueueData.Builder().setItems(queue).setStartIndex(0).build()
                val load = MediaLoadRequestData.Builder()
                    .setQueueData(data)
                    .setCurrentTime(position)
                    .setAutoplay(true)
                    .build()
                session.remoteMediaClient?.load(load)
                remotePlaying = true
            } catch (_: IOException) {
                player.error = t("cast.unreachable")
                stop()
            }
        }
    }

    private suspend fun info(item: MediaItem): MediaInfo {
        val id = item.mediaId.toLong()
        val links = app.api.playLinks(id)
        val meta = CastMetadata(CastMetadata.MEDIA_TYPE_MUSIC_TRACK).apply {
            putString(CastMetadata.KEY_TITLE, item.mediaMetadata.title?.toString() ?: "")
            putString(CastMetadata.KEY_ARTIST, item.mediaMetadata.artist?.toString() ?: "")
            putString(CastMetadata.KEY_ALBUM_TITLE, item.mediaMetadata.albumTitle?.toString() ?: "")
        }
        // Cast devices fetch it themselves, and won't accept the server's own certificate.
        return MediaInfo.Builder(app.account.plainAddress + links.url)
            .setStreamType(MediaInfo.STREAM_TYPE_BUFFERED)
            .setContentType(links.mime.substringBefore(';'))
            .setMetadata(meta)
            .build()
    }

    fun toggleRemote() {
        val client = cast?.sessionManager?.currentCastSession?.remoteMediaClient ?: return
        if (remotePlaying) client.pause() else client.play()
        remotePlaying = !remotePlaying
    }

    fun nextRemote() {
        cast?.sessionManager?.currentCastSession?.remoteMediaClient?.queueNext(null)
    }

    private fun finished() {
        castingTo = null
        remotePlaying = false
    }
}
