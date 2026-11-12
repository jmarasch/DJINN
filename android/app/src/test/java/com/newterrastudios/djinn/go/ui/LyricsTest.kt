package com.newterrastudios.djinn.go.ui

import com.newterrastudios.djinn.go.data.LyricLine
import com.newterrastudios.djinn.go.data.Lyrics
import kotlinx.serialization.json.Json
import org.junit.Assert.assertEquals
import org.junit.Test

class LyricsTest {
    private val lines = listOf(LyricLine(1000, "One"), LyricLine(4000, "Two"), LyricLine(9000, "Three"))

    @Test
    fun theLineBeingSungIsTheLastOneStarted() {
        assertEquals(-1, lineAt(lines, 0))
        assertEquals(0, lineAt(lines, 1000))
        assertEquals(0, lineAt(lines, 3999))
        assertEquals(1, lineAt(lines, 4000))
        assertEquals(2, lineAt(lines, 60_000))
        assertEquals(-1, lineAt(emptyList(), 5000))
    }

    @Test
    fun theServersLyricsRead() {
        val json = Json { ignoreUnknownKeys = true }
        val lyrics = json.decodeFromString<Lyrics>(
            """{"lines":[{"ms":500,"text":"Hi"}],"plain":"Hi","instrumental":false,"source":"lrc"}""",
        )
        assertEquals(500L, lyrics.lines?.first()?.ms)
        assertEquals("lrc", lyrics.source)
    }
}
