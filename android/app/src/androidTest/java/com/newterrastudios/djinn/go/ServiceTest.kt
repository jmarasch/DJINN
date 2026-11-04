package com.newterrastudios.djinn.go

import android.content.ComponentName
import android.content.Intent
import android.os.Looper
import android.view.KeyEvent
import android.widget.FrameLayout
import android.widget.TextView
import androidx.media3.common.MediaItem
import androidx.media3.common.util.UnstableApi
import androidx.media3.session.MediaBrowser
import androidx.media3.session.MediaButtonReceiver
import androidx.media3.session.SessionToken
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.newterrastudios.djinn.go.player.NowPlayingWidget
import com.newterrastudios.djinn.go.player.PlaybackService
import com.newterrastudios.djinn.go.player.albumId
import com.newterrastudios.djinn.go.player.artistId
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import java.util.concurrent.Callable
import java.util.concurrent.FutureTask
import java.util.concurrent.TimeUnit

/**
 * What a car, a headset and the widget see of the player. Needs the app signed
 * in to a server with a library on it, so it isn't part of a plain build:
 * install both APKs and run with `am instrument`.
 */
@UnstableApi
@RunWith(AndroidJUnit4::class)
class ServiceTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val context get() = instrumentation.targetContext
    private lateinit var browser: MediaBrowser

    /** Runs on the thread the browser belongs to. */
    private fun <T> main(block: () -> T): T {
        val task = FutureTask(Callable { block() })
        instrumentation.runOnMainSync(task)
        return task.get(30, TimeUnit.SECONDS)
    }

    private fun waitUntil(what: String, condition: () -> Boolean) {
        val end = System.currentTimeMillis() + 20_000
        while (System.currentTimeMillis() < end) {
            if (main(condition)) return
            Thread.sleep(200)
        }
        throw AssertionError("never: $what")
    }

    private fun children(id: String): List<MediaItem> {
        val future = main { browser.getChildren(id, 0, 100, null) }
        return future.get(30, TimeUnit.SECONDS).value.orEmpty().toList()
    }

    @Before
    fun connect() {
        val token = SessionToken(context, ComponentName(context, PlaybackService::class.java))
        browser = main {
            MediaBrowser.Builder(context, token).setApplicationLooper(Looper.getMainLooper()).buildAsync()
        }.get(30, TimeUnit.SECONDS)
    }

    @After
    fun disconnect() {
        main {
            browser.stop()
            browser.clearMediaItems()
            browser.release()
        }
    }

    @Test
    fun theLibraryCanBeBrowsedAsACarDoes() {
        val root = main { browser.getLibraryRoot(null) }.get(30, TimeUnit.SECONDS).value
        assertEquals("root", root?.mediaId)
        val top = children("root")
        assertEquals(listOf("Lately played", "On this phone", "Playlists", "Albums"), top.map { it.mediaMetadata.title.toString() })
        assertTrue(top.all { it.mediaMetadata.isBrowsable == true && it.mediaMetadata.isPlayable == false })

        val albums = children("albums")
        assertTrue("albums listed", albums.isNotEmpty())
        val songs = children(albums.first().mediaId)
        assertTrue("songs listed", songs.isNotEmpty())
        assertTrue("songs have ids", songs.all { it.mediaId.toLongOrNull() != null })
    }

    @Test
    fun aCarPlaysASongFromItsIdAlone() {
        val album = children("albums").first()
        val song = children(album.mediaId).first()
        main {
            browser.setMediaItem(MediaItem.Builder().setMediaId(song.mediaId).build())
            browser.prepare()
            browser.play()
        }
        waitUntil("the song plays") { browser.isPlaying }
        assertEquals(song.mediaMetadata.title.toString(), main { browser.mediaMetadata.title.toString() })
    }

    @Test
    fun aSongsArtistAndAlbumReachTheScreens() {
        val songs = children(children("albums").first().mediaId)
        assertNotNull("browsed songs have an artist", songs.first().artistId)
        assertNotNull("browsed songs have an album", songs.first().albumId)
        main {
            browser.setMediaItems(songs)
            browser.prepare()
            browser.play()
        }
        waitUntil("the album plays") { browser.isPlaying }
        val playing = main { browser.currentMediaItem }
        assertEquals(songs.first().artistId, playing?.artistId)
        assertEquals(songs.first().albumId, playing?.albumId)
    }

    @Test
    fun aSongPlayedFromItsIdAloneKnowsItsArtistAndAlbum() {
        val song = children(children("albums").first().mediaId).first()
        main {
            browser.setMediaItem(MediaItem.Builder().setMediaId(song.mediaId).build())
            browser.prepare()
            browser.play()
        }
        waitUntil("the song plays") { browser.isPlaying }
        val playing = main { browser.currentMediaItem }
        assertEquals(song.artistId, playing?.artistId)
        assertEquals(song.albumId, playing?.albumId)
    }

    @Test
    fun searchFindsSongs() {
        main { browser.search("Long Album", null) }.get(30, TimeUnit.SECONDS)
        val found = main { browser.getSearchResult("Long Album", 0, 50, null) }.get(30, TimeUnit.SECONDS).value
        assertTrue("found songs", !found.isNullOrEmpty())
    }

    @Test
    fun aVoiceRequestPlaysWhatItFinds() {
        main {
            browser.setMediaItem(
                MediaItem.Builder()
                    .setRequestMetadata(MediaItem.RequestMetadata.Builder().setSearchQuery("Long Album").build())
                    .build(),
            )
            browser.prepare()
            browser.play()
        }
        waitUntil("something from the search plays") { browser.isPlaying }
        assertTrue(main { browser.mediaMetadata.title.toString() }.startsWith("Long Album"))
    }

    @Test
    fun theWidgetsButtonsWorkThePlayer() {
        // The same broadcast the widget's buttons send.
        fun press(code: Int) = context.sendBroadcast(
            Intent(Intent.ACTION_MEDIA_BUTTON)
                .setClass(context, MediaButtonReceiver::class.java)
                .putExtra(Intent.EXTRA_KEY_EVENT, KeyEvent(KeyEvent.ACTION_DOWN, code)),
        )
        val album = children("albums").first { it.mediaMetadata.title.toString() == "Long Album" }
        val songs = children(album.mediaId)
        main {
            browser.setMediaItems(songs)
            browser.prepare()
            browser.play()
        }
        waitUntil("the album plays") { browser.isPlaying }
        press(KeyEvent.KEYCODE_MEDIA_PLAY_PAUSE)
        waitUntil("the play button pauses") { !browser.isPlaying }
        press(KeyEvent.KEYCODE_MEDIA_PLAY_PAUSE)
        waitUntil("the play button plays") { browser.isPlaying }
        val before = main { browser.currentMediaItemIndex }
        context.sendBroadcast(Intent(context, NowPlayingWidget::class.java).setAction(NowPlayingWidget.ACTION_NEXT))
        waitUntil("the next button moves on") { browser.currentMediaItemIndex == before + 1 }
    }

    @Test
    fun theWidgetLayoutInflatesAndShowsTheSong() {
        main {
            val parent = FrameLayout(context)
            val views = NowPlayingWidget.views(context, "A song", "An artist", true).apply(context, parent)
            assertEquals("A song", views.findViewById<TextView>(R.id.widget_title).text.toString())
            assertEquals("An artist", views.findViewById<TextView>(R.id.widget_artist).text.toString())
            assertTrue(views.findViewById<android.view.View>(R.id.widget_play).hasOnClickListeners())
            assertTrue(views.findViewById<android.view.View>(R.id.widget_next).hasOnClickListeners())
            assertFalse(views.findViewById<TextView>(R.id.widget_title).text.isEmpty())
        }
    }
}
