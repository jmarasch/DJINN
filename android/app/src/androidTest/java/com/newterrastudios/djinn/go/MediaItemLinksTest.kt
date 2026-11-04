package com.newterrastudios.djinn.go

import androidx.media3.common.MediaItem
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.newterrastudios.djinn.go.data.KIND_DOWNLOAD
import com.newterrastudios.djinn.go.data.OfflineSong
import com.newterrastudios.djinn.go.data.Track
import com.newterrastudios.djinn.go.player.albumId
import com.newterrastudios.djinn.go.player.artistId
import com.newterrastudios.djinn.go.player.toKeptItem
import com.newterrastudios.djinn.go.player.toMediaItem
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import org.junit.runner.RunWith

/** Which songs Now Playing can link from. Needs a device for Bundle, but no sign-in. */
@RunWith(AndroidJUnit4::class)
class MediaItemLinksTest {
    private val api
        get() = (InstrumentationRegistry.getInstrumentation().targetContext.applicationContext as DjinnApp).api

    @Test
    fun aSongCarriesItsArtistAndAlbum() {
        val item = Track(1, "A song", artistId = 2, albumId = 3).toMediaItem(api)
        assertEquals(2L, item.artistId)
        assertEquals(3L, item.albumId)
    }

    @Test
    @Suppress("DEPRECATION")
    fun theIdsSurviveBeingSent() {
        val item = Track(1, "A song", artistId = 2, albumId = 3).toMediaItem(api)
        val sent = MediaItem.fromBundle(item.toBundle())
        assertEquals(2L, sent.artistId)
        assertEquals(3L, sent.albumId)
    }

    @Test
    fun aSongWithoutThemHasNoLinks() {
        val item = Track(1, "A song").toMediaItem(api)
        assertNull(item.artistId)
        assertNull(item.albumId)
    }

    @Test
    fun anotherServersSongHasNoLinks() {
        val item = OfflineSong(
            server = "another",
            id = 1,
            title = "A song",
            artist = "A band",
            album = "An album",
            artistId = 2,
            albumId = 3,
            trackNo = null,
            durationMs = null,
            artHash = null,
            kind = KIND_DOWNLOAD,
            path = "song.mp3",
            bytes = 0,
            gain = 1f,
            version = "1",
            converted = false,
            addedAt = 0,
        ).toKeptItem()
        assertNull(item.artistId)
        assertNull(item.albumId)
    }
}
