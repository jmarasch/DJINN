package com.newterrastudios.djinn.go.ui

import com.newterrastudios.djinn.go.data.Jump
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class JumpStripTest {
    private fun at(entries: List<JumpEntry>, label: String) = entries.first { it.label == label }.offset

    @Test
    fun `a list of names gets its letters from where each one starts`() {
        val names = listOf("80s pop", "ambient", "art rock", "écossaise", "jazz", "rock")
        assertEquals(
            listOf(Jump("#", 0), Jump("A", 1), Jump("E", 3), Jump("J", 4), Jump("R", 5)),
            letterJumps(names),
        )
    }

    @Test
    fun `a letter missing from a list of names goes to the next one`() {
        val entries = jumpEntries(letterJumps(listOf("ambient", "jazz", "rock")), "name")
        assertEquals(1, at(entries, "B"))
        assertEquals(2, at(entries, "K"))
    }

    @Test
    fun `every letter is on the strip, even with nothing under it`() {
        val entries = jumpEntries(listOf(Jump("B", 0), Jump("D", 10)), "name")
        assertEquals("ABCDEFGHIJKLMNOPQRSTUVWXYZ".map { it.toString() }, entries.map { it.label })
    }

    @Test
    fun `a missing letter goes to the next one there is`() {
        val entries = jumpEntries(listOf(Jump("B", 0), Jump("D", 10), Jump("Y", 40)), "name")
        assertEquals(0, at(entries, "A"))
        assertEquals(10, at(entries, "C"))
        assertEquals(10, at(entries, "D"))
        assertEquals(40, at(entries, "E"))
    }

    @Test
    fun `letters after the last one go to the last one`() {
        val entries = jumpEntries(listOf(Jump("B", 0), Jump("D", 10)), "name")
        assertEquals(10, at(entries, "Z"))
    }

    @Test
    fun `names that aren't letters come first, when there are any`() {
        val entries = jumpEntries(listOf(Jump("#", 0), Jump("A", 3)), "name")
        assertEquals("#", entries.first().label)
        assertEquals(0, entries.first().offset)
    }

    @Test
    fun `newest first runs down through every year`() {
        val entries = jumpEntries(listOf(Jump("2003", 0), Jump("2001", 7)), "year")
        assertEquals(listOf("2003", "2002", "2001"), entries.map { it.label })
        assertEquals(listOf(0, 7, 7), entries.map { it.offset })
    }

    @Test
    fun `oldest first runs up through every year`() {
        val entries = jumpEntries(listOf(Jump("2001", 0), Jump("2003", 7)), "oldest")
        assertEquals(listOf("2001", "2002", "2003"), entries.map { it.label })
        assertEquals(listOf(0, 7, 7), entries.map { it.offset })
    }

    @Test
    fun `an order with no jumps has no strip`() {
        assertTrue(jumpEntries(emptyList(), "added").isEmpty())
    }

    @Test
    fun `when everything fits, everything gets a label`() {
        val entries = jumpEntries(listOf(Jump("A", 0)), "name")
        assertEquals(entries.indices.toList(), labelledEntries(entries, 100))
    }

    @Test
    fun `letters thin out evenly and never overflow`() {
        val entries = jumpEntries(listOf(Jump("A", 0)), "name")
        val shown = labelledEntries(entries, 10)
        assertTrue(shown.size <= 10)
        assertEquals(0, shown.first())
        assertEquals(setOf(shown[1] - shown[0]), shown.zipWithNext { a, b -> b - a }.toSet())
    }

    @Test
    fun `years keep the first and last, with round ones between`() {
        val entries = jumpEntries(listOf(Jump("1950", 0), Jump("2020", 50)), "oldest")
        val shown = labelledEntries(entries, 12).map { entries[it].label }
        assertEquals("1950", shown.first())
        assertEquals("2020", shown.last())
        assertTrue(shown.size <= 12)
        assertTrue(shown.drop(1).dropLast(1).all { it.toInt() % 10 == 0 })
    }
}
