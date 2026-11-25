package com.newterrastudios.djinn.go.ui

import com.newterrastudios.djinn.go.data.GenreWeb
import com.newterrastudios.djinn.go.data.WebGenre
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class GenreLayoutTest {
    // Two families: Rock (hard rock, glam rock under it) and Metal (glam
    // metal), with glam metal grown out of glam rock and grunge reacting
    // against it; jazz has no songs.
    private val web = GenreWeb(
        genres = listOf(
            WebGenre("Rock", songsBelow = 100),
            WebGenre("Hard rock", parent = 0, songsBelow = 60),
            WebGenre("Glam rock", parent = 1, songsBelow = 20),
            WebGenre("Metal", from = listOf(1), songsBelow = 50),
            WebGenre("Glam metal", parent = 3, from = listOf(2), songsBelow = 30),
            WebGenre("Grunge", parent = 0, against = listOf(4), songsBelow = 10),
            WebGenre("Jazz", songsBelow = 0),
        ),
    )

    @Test
    fun theTopLevelIsTheFamiliesWithTheirTies() {
        val top = layOutWeb(web, null, every = false)
        assertEquals(listOf("Rock", "Metal"), top.dots.map { it.name })
        // Rock and Metal are tied by several links, drawn as one line.
        assertEquals(1, top.lines.size)
        assertEquals(3, layOutWeb(web, null, every = true).dots.size)
    }

    @Test
    fun aGenreHasItsParentKindsAndLinksAroundIt() {
        val view = layOutWeb(web, "Glam metal", every = false)
        assertEquals("Glam metal", view.dots.first().name)
        assertEquals(setOf("Glam metal", "Metal", "Glam rock", "Grunge"), view.dots.map { it.name }.toSet())
        val kinds = view.lines.map { view.dots[it.a].name to view.dots[it.b].name to it.kind }.toSet()
        assertTrue(("Metal" to "Glam metal" to LineKind.TREE) in kinds)
        // Arrows run from the older genre to the newer one...
        assertTrue(("Glam rock" to "Glam metal" to LineKind.FROM) in kinds)
        // ...and from a reaction to what it reacted against.
        assertTrue(("Grunge" to "Glam metal" to LineKind.AGAINST) in kinds)
        // Genres from another family are told apart.
        assertEquals(false, view.dots.single { it.name == "Glam rock" }.home)
    }

    @Test
    fun onlyTheCentersLinesAreDrawn() {
        val view = layOutWeb(web, "Hard rock", every = false)
        assertTrue(view.lines.all { it.a == 0 || it.b == 0 })
        assertEquals(listOf("Rock", "Hard rock", "Glam rock"), lineage(web, "Glam rock"))
    }

    @Test
    fun theMiddleGenreStaysInTheMiddle() {
        val view = layOutWeb(web, "Rock", every = false)
        assertEquals(0f, view.dots.first().x)
        assertEquals(0f, view.dots.first().y)
    }

    @Test
    fun aGenreNoLongerThereFallsBackToTheTop() {
        assertEquals(listOf("Rock", "Metal"), layOutWeb(web, "gone", every = false).dots.map { it.name })
    }

    @Test
    fun theSameLibraryAlwaysLooksTheSame() {
        val (one, two) = layOutWeb(web, "Hard rock", false) to layOutWeb(web, "Hard rock", false)
        assertEquals(one.dots.map { it.x to it.y }, two.dots.map { it.x to it.y })
    }

    @Test
    fun tappingAGenreOnTheWayGoesBackToIt() {
        assertEquals(listOf("a", "b", "c"), pathAfter(listOf("a", "b"), "c"))
        assertEquals(listOf("a"), pathAfter(listOf("a", "b"), "a"))
    }

    @Test
    fun aTapFindsTheDotOrItsLabel() {
        val layout = layOutWeb(web, null, every = false)
        val dot = layout.dots.first()
        assertNotNull(dotAt(layout, dot.x, dot.y))
        assertNotNull(dotAt(layout, dot.x, dot.y + dot.r + dot.font))
        assertNull(dotAt(layout, dot.x + 900, dot.y))
    }
}
