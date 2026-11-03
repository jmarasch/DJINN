package com.newterrastudios.djinn.go.data

import org.junit.Assert.assertEquals
import org.junit.Test

class ImportTextTest {
    @Test
    fun readsWhatMusicAppsExport() {
        assertEquals("Björk - Jóga", importText("Björk - Jóga".toByteArray(Charsets.UTF_8)))
        // Apple Music's text export: UTF-16 with a mark saying which way round.
        val little = byteArrayOf(0xff.toByte(), 0xfe.toByte()) + "Name\tArtist".toByteArray(Charsets.UTF_16LE)
        assertEquals("Name\tArtist", importText(little))
        val big = byteArrayOf(0xfe.toByte(), 0xff.toByte()) + "Name\tArtist".toByteArray(Charsets.UTF_16BE)
        assertEquals("Name\tArtist", importText(big))
    }

    @Test
    fun aMarkOnUtf8DoesntReachTheServer() {
        val marked = byteArrayOf(0xef.toByte(), 0xbb.toByte(), 0xbf.toByte()) + "#EXTM3U".toByteArray()
        assertEquals("#EXTM3U", importText(marked))
    }

    @Test
    fun anEmptyFileIsJustEmpty() {
        assertEquals("", importText(ByteArray(0)))
    }
}
