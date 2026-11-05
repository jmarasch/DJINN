package com.newterrastudios.djinn.go.ui

import com.newterrastudios.djinn.go.data.GenreLink
import com.newterrastudios.djinn.go.data.GenreList
import com.newterrastudios.djinn.go.data.GenreSummary
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Test

class GenreLayoutTest {
    private val list = GenreList(
        genres = listOf(
            GenreSummary("a", 100),
            GenreSummary("b", 50),
            GenreSummary("c", 20),
            GenreSummary("d", 10),
            GenreSummary("e", 5),
        ),
        // a-b-c-d in a chain; e on its own.
        links = listOf(GenreLink(0, 1, 0.9f), GenreLink(1, 2, 0.8f), GenreLink(2, 3, 0.7f)),
        topLinks = listOf(GenreLink(0, 1, 0.9f)),
    )

    @Test
    fun zoomingShowsNeighboursAndTheirNeighboursButNoFurther() {
        assertEquals(listOf(0, 1, 2), around(list.links, 0))
        assertEquals(listOf(4), around(list.links, 4))
    }

    @Test
    fun theTopLevelIsTheBiggestWithTheirOwnLinks() {
        val top = layOutGenres(list, null, 2)
        assertEquals(setOf("a", "b"), top.dots.map { it.name }.toSet())
        assertEquals(1, top.lines.size)
    }

    @Test
    fun aZoomedViewUsesEveryLinkAmongWhatItShows() {
        val zoomed = layOutGenres(list, "b", 2)
        assertEquals(setOf("a", "b", "c", "d"), zoomed.dots.map { it.name }.toSet())
        assertEquals(3, zoomed.lines.size)
        assertEquals("b", zoomed.dots.first().name)
    }

    @Test
    fun aGenreNoLongerThereFallsBackToTheTop() {
        assertEquals(2, layOutGenres(list, "gone", 2).dots.size)
    }

    @Test
    fun theSameLibraryAlwaysLooksTheSame() {
        val (one, two) = layOutGenres(list, null, 5) to layOutGenres(list, null, 5)
        assertEquals(one.dots.map { it.x to it.y }, two.dots.map { it.x to it.y })
    }

    @Test
    fun tappingAGenreOnTheWayGoesBackToIt() {
        assertEquals(listOf("a", "b", "c"), pathAfter(listOf("a", "b"), "c"))
        assertEquals(listOf("a"), pathAfter(listOf("a", "b"), "a"))
    }

    @Test
    fun aTapFindsTheDotOrItsLabel() {
        val layout = layOutGenres(list, null, 1)
        val dot = layout.dots.single()
        assertNotNull(dotAt(layout, dot.x, dot.y))
        assertNotNull(dotAt(layout, dot.x, dot.y + dot.r + dot.font))
        assertNull(dotAt(layout, dot.x + 500, dot.y))
    }
}
