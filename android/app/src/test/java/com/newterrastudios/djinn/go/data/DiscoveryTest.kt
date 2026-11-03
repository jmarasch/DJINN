package com.newterrastudios.djinn.go.data

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import java.net.InetAddress

class DiscoveryTest {
    @Test
    fun `addresses are ready to use`() {
        assertEquals("http://192.168.1.20:8067", foundAddress("192.168.1.20", 8067))
        assertEquals("http://[fd00::1]:8067", foundAddress("fd00::1", 8067))
        assertEquals("http://[fd00::1]:8067", foundAddress("fd00::1%wlan0", 8067))
    }

    @Test
    fun `a found address is what typing one would give`() {
        assertEquals("http://192.168.1.20:8067", serverAddress(foundAddress("192.168.1.20", 8067)))
    }

    @Test
    fun `servers are listed by name, and hearing one again updates it`() {
        var list = emptyList<FoundServer>()
        list = withServer(list, FoundServer("b", "zebra", "http://10.0.0.2:8067"))
        list = withServer(list, FoundServer("a", "Attic", "http://10.0.0.1:8067"))
        list = withServer(list, FoundServer("c", "basement", "http://10.0.0.3:8067"))
        assertEquals(listOf("Attic", "basement", "zebra"), list.map { it.name })

        list = withServer(list, FoundServer("b", "Yard", "http://10.0.0.9:8067"))
        assertEquals(listOf("Attic", "basement", "Yard"), list.map { it.name })
        assertEquals("http://10.0.0.9:8067", list.last().address)
    }

    @Test
    fun `two servers can share a name`() {
        var list = emptyList<FoundServer>()
        list = withServer(list, FoundServer("a", "DJINN", "http://10.0.0.2:8067"))
        list = withServer(list, FoundServer("b", "DJINN", "http://10.0.0.1:8067"))
        assertEquals(listOf("http://10.0.0.1:8067", "http://10.0.0.2:8067"), list.map { it.address })
    }

    @Test
    fun `IPv4 is preferred and link-local IPv6 is left out`() {
        val v4 = InetAddress.getByName("192.168.1.20")
        val linkLocal = InetAddress.getByName("fe80::1")
        val v6 = InetAddress.getByName("fd00::1")
        assertEquals("192.168.1.20", bestHost(listOf(linkLocal, v6, v4)))
        assertEquals(v6.hostAddress, bestHost(listOf(linkLocal, v6)))
        assertNull(bestHost(listOf(linkLocal)))
        assertNull(bestHost(emptyList()))
    }
}
