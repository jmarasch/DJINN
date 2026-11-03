package com.newterrastudios.djinn.go.data

import kotlinx.serialization.json.Json
import okhttp3.HttpUrl.Companion.toHttpUrl
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class GenreApiTest {
    private val json = Json { ignoreUnknownKeys = true }

    @Test
    fun `spaces in a genre's name are escaped, not a plus`() {
        assertEquals("/api/genres/hair%20metal", genrePath("hair metal"))
    }

    @Test
    fun `a plain name is left alone`() {
        assertEquals("/api/genres/rock", genrePath("rock"))
    }

    @Test
    fun `any name arrives as one piece of the path, unchanged`() {
        listOf("hair metal", "r&b", "drum/bass", "a+b", "50% off", "música", "what?", "#1").forEach { name ->
            val url = ("http://djinn.local:8067" + genrePath(name)).toHttpUrl()
            assertEquals(listOf("api", "genres", name), url.pathSegments)
            assertNull(url.query)
            assertNull(url.fragment)
        }
    }

    @Test
    fun `the genre list reads what the server sends`() {
        val list = json.decodeFromString<GenreList>(
            """{"genres":[{"name":"hair metal","songs":12}],"links":[{"a":0,"b":0,"strength":0.5}]}""",
        )
        assertEquals(listOf(GenreSummary("hair metal", 12)), list.genres)
    }

    @Test
    fun `a genre's page reads what the server sends`() {
        val page = json.decodeFromString<GenrePage>(
            """
            {"name":"rock","total":300,
             "tracks":[{"id":1,"title":"A song","artist_id":2,"album_id":3}],
             "artists":[{"id":2,"name":"A band","art_hash":null,"track_count":9}],
             "related":[{"name":"hard rock","songs":40,"strength":0.8}]}
            """,
        )
        assertEquals(300L, page.total)
        assertEquals(2L, page.tracks.single().artistId)
        assertEquals("A band", page.artists.single().name)
        assertEquals(GenreSummary("hard rock", 40), page.related.single())
    }
}
