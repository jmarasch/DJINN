package com.newterrastudios.djinn.go.ui

import com.newterrastudios.djinn.go.data.Album
import org.junit.Assert.assertEquals
import org.junit.Test

class ArtistAlbumSortTest {
    private fun album(id: Long, title: String, year: Int?) = Album(id = id, title = title, artistId = 1, artist = "Kitchen", year = year)

    private val albums = listOf(
        album(1, "Garden Party", 2011),
        album(2, "basement tapes", null),
        album(3, "Kitchen Lights", 1994),
        album(4, "Porch Songs", 2003),
        album(5, "Attic", 2003),
    )

    private fun titles(sort: String) = sortArtistAlbums(albums, sort).map { it.title }

    @Test
    fun `newest first, albums with no year last`() {
        assertEquals(listOf("Garden Party", "Attic", "Porch Songs", "Kitchen Lights", "basement tapes"), titles("year"))
    }

    @Test
    fun `oldest first, albums with no year last`() {
        assertEquals(listOf("Kitchen Lights", "Attic", "Porch Songs", "Garden Party", "basement tapes"), titles("oldest"))
    }

    @Test
    fun `A to Z ignores capitals`() {
        assertEquals(listOf("Attic", "basement tapes", "Garden Party", "Kitchen Lights", "Porch Songs"), titles("name"))
    }
}
