package com.newterrastudios.djinn.go.player

import androidx.media3.common.C
import org.junit.Assert.assertEquals
import org.junit.Test

class CacheAheadTest {
    /** A queue of [size] songs in [order], ending or (with [wrap]) coming round. */
    private fun nextIn(order: List<Int>, wrap: Boolean): (Int) -> Int = { index ->
        val at = order.indexOf(index)
        when {
            at + 1 < order.size -> order[at + 1]
            wrap -> order[0]
            else -> C.INDEX_UNSET
        }
    }

    @Test
    fun takesTheNextSongsInOrder() {
        assertEquals(listOf(3, 4, 5), aheadIndices(2, 3, nextIn((0..9).toList(), wrap = false)))
    }

    @Test
    fun stopsAtTheEndOfTheQueue() {
        assertEquals(listOf(8, 9), aheadIndices(7, 5, nextIn((0..9).toList(), wrap = false)))
    }

    @Test
    fun followsTheShuffleOrder() {
        assertEquals(listOf(0, 7), aheadIndices(4, 2, nextIn(listOf(2, 4, 0, 7, 1), wrap = false)))
    }

    @Test
    fun repeatingAllStopsBeforeComingRound() {
        assertEquals(listOf(2, 0), aheadIndices(1, 10, nextIn(listOf(0, 1, 2), wrap = true)))
    }

    @Test
    fun offFetchesNothing() {
        assertEquals(emptyList<Int>(), aheadIndices(0, 0, nextIn((0..9).toList(), wrap = false)))
    }
}
