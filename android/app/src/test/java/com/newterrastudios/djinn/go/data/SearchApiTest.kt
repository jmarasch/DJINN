package com.newterrastudios.djinn.go.data

import kotlinx.serialization.json.Json
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class SearchApiTest {
    private val json = Json { ignoreUnknownKeys = true }

    @Test
    fun `a search says when the AI reader might help`() {
        val found = json.decodeFromString<SearchResults>("""{"tracks":[],"described":null,"ask":true}""")
        assertTrue(found.ask)
    }

    @Test
    fun `an older server never offers the AI reader`() {
        assertFalse(json.decodeFromString<SearchResults>("""{"tracks":[]}""").ask)
    }

    @Test
    fun `the AI reader can answer with nothing`() {
        assertNull(json.decodeFromString<Described?>("null"))
    }

    @Test
    fun `the AI reader's answer is a description`() {
        val read = json.decodeFromString<Described?>("""{"summary":["chill","house"],"tracks":[],"read_by_ai":true}""")
        assertEquals(listOf("chill", "house"), read?.summary)
    }

    @Test
    fun `the server says whether YouTube links are on`() {
        assertTrue(json.decodeFromString<YoutubeLinks>("""{"has_key":true}""").hasKey)
        assertFalse(json.decodeFromString<YoutubeLinks>("""{"has_key":false}""").hasKey)
    }
}
