package com.newterrastudios.djinn.go.data

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class DiscoveryTest {
    private val heard = listOf(
        FoundServer("aaa", "Kitchen", "http://192.168.1.20:8067"),
        FoundServer("bbb", "Kitchen", "http://192.168.1.31:8067"),
    )

    @Test
    fun theServerInUseIsKnownByItsIdNotItsName() {
        assertEquals("http://192.168.1.31:8067", announcedAs(heard, "bbb")?.address)
        assertNull(announcedAs(heard, "ccc"))
    }

    @Test
    fun aMovedServerIsUpdatedNotAdded() {
        val moved = withServer(heard, FoundServer("aaa", "Kitchen", "http://192.168.1.44:8067"))
        assertEquals(2, moved.size)
        assertEquals("http://192.168.1.44:8067", announcedAs(moved, "aaa")?.address)
    }
}
