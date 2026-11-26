package com.newterrastudios.djinn.go.ui

import com.newterrastudios.djinn.go.data.Album
import org.junit.Assert.assertEquals
import org.junit.Test

class ArtistAlbumSortTest {
    private fun album(id: Long, title: String, year: Int?, added: Long?) =
        Album(id = id, title = title, artistId = 1, artist = "Kitchen", year = year, added = added)

    private val albums = listOf(
        album(1, "Garden Party", 2011, 300),
        album(2, "basement tapes", null, null),
        album(3, "Kitchen Lights", 1994, 500),
        album(4, "Porch Songs", 2003, 100),
        album(5, "Attic", 2003, 200),
    )

    private fun titles(sort: String) = sortArtistAlbums(albums, sort).map { it.title }

    @Test
    fun `release, newest first, albums with no year last`() {
        assertEquals(listOf("Garden Party", "Attic", "Porch Songs", "Kitchen Lights", "basement tapes"), titles("release"))
    }

    @Test
    fun `release reversed, oldest first, albums with no year still last`() {
        assertEquals(listOf("Kitchen Lights", "Attic", "Porch Songs", "Garden Party", "basement tapes"), titles("release_old"))
    }

    @Test
    fun `added, newest first, and reversed`() {
        assertEquals(listOf("Kitchen Lights", "Garden Party", "Attic", "Porch Songs", "basement tapes"), titles("added"))
        assertEquals(listOf("Porch Songs", "Attic", "Garden Party", "Kitchen Lights", "basement tapes"), titles("added_old"))
    }

    @Test
    fun `release goes by the first release year when MusicBrainz knows it`() {
        val reissue = Album(id = 6, title = "Reissue", artistId = 1, artist = "Kitchen", year = 2020, firstYear = 1990)
        assertEquals(listOf("Garden Party", "Attic", "Porch Songs", "Kitchen Lights", "Reissue", "basement tapes"),
            sortArtistAlbums(albums + reissue, "release").map { it.title })
    }

    @Test
    fun `A to Z ignores capitals, and reversed`() {
        assertEquals(listOf("Attic", "basement tapes", "Garden Party", "Kitchen Lights", "Porch Songs"), titles("name"))
        assertEquals(listOf("Porch Songs", "Kitchen Lights", "Garden Party", "basement tapes", "Attic"), titles("name_rev"))
    }
}
