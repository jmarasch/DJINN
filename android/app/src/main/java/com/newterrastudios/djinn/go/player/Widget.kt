package com.newterrastudios.djinn.go.player

import android.app.PendingIntent
import android.appwidget.AppWidgetManager
import android.appwidget.AppWidgetProvider
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.view.KeyEvent
import android.widget.RemoteViews
import androidx.media3.common.util.UnstableApi
import androidx.core.content.ContextCompat
import androidx.media3.session.MediaButtonReceiver
import androidx.media3.session.MediaController
import androidx.media3.session.SessionToken
import com.newterrastudios.djinn.go.MainActivity
import com.newterrastudios.djinn.go.R

/** The song playing, on the home screen, with play and next. */
@UnstableApi
class NowPlayingWidget : AppWidgetProvider() {
    /** Next, which the media button receiver won't pass on (it only starts playback). */
    override fun onReceive(context: Context, intent: Intent) {
        super.onReceive(context, intent)
        if (intent.action != ACTION_NEXT) return
        val finished = goAsync()
        // A receiver's own context isn't allowed to bind to the player.
        val app = context.applicationContext
        val token = SessionToken(app, ComponentName(app, PlaybackService::class.java))
        val connecting = MediaController.Builder(app, token).buildAsync()
        connecting.addListener({
            runCatching {
                connecting.get().run {
                    seekToNextMediaItem()
                    release()
                }
            }
            finished.finish()
        }, ContextCompat.getMainExecutor(context))
    }

    override fun onUpdate(context: Context, manager: AppWidgetManager, ids: IntArray) {
        manager.updateAppWidget(ids, views(context, last.title, last.artist, last.playing))
    }

    private data class Showing(val title: String?, val artist: String?, val playing: Boolean)

    companion object {
        internal const val ACTION_NEXT = "com.newterrastudios.djinn.go.WIDGET_NEXT"
        private var last = Showing(null, null, false)

        /** From the player, whenever what it's doing changes. */
        fun show(context: Context, title: String?, artist: String?, playing: Boolean) {
            val now = Showing(title, artist, playing)
            if (now == last) return
            last = now
            val manager = AppWidgetManager.getInstance(context)
            val ids = manager.getAppWidgetIds(ComponentName(context, NowPlayingWidget::class.java))
            if (ids.isNotEmpty()) manager.updateAppWidget(ids, views(context, title, artist, playing))
        }

        internal fun views(context: Context, title: String?, artist: String?, playing: Boolean): RemoteViews {
            val views = RemoteViews(context.packageName, R.layout.widget)
            views.setTextViewText(R.id.widget_title, title ?: "DJINN GO")
            views.setTextViewText(R.id.widget_artist, artist ?: "Tap to open")
            views.setImageViewResource(
                R.id.widget_play,
                if (playing) android.R.drawable.ic_media_pause else android.R.drawable.ic_media_play,
            )
            views.setOnClickPendingIntent(R.id.widget_play, key(context, KeyEvent.KEYCODE_MEDIA_PLAY_PAUSE))
            views.setOnClickPendingIntent(
                R.id.widget_next,
                PendingIntent.getBroadcast(
                    context,
                    0,
                    Intent(context, NowPlayingWidget::class.java).setAction(ACTION_NEXT),
                    PendingIntent.FLAG_IMMUTABLE,
                ),
            )
            views.setOnClickPendingIntent(
                R.id.widget_text,
                PendingIntent.getActivity(
                    context,
                    0,
                    Intent(context, MainActivity::class.java),
                    PendingIntent.FLAG_IMMUTABLE,
                ),
            )
            return views
        }

        /** As if a headset button were pressed; it also wakes the player. */
        private fun key(context: Context, code: Int): PendingIntent {
            val intent = Intent(Intent.ACTION_MEDIA_BUTTON)
                .setClass(context, MediaButtonReceiver::class.java)
                .putExtra(Intent.EXTRA_KEY_EVENT, KeyEvent(KeyEvent.ACTION_DOWN, code))
            return PendingIntent.getBroadcast(context, code, intent, PendingIntent.FLAG_IMMUTABLE)
        }
    }
}
