package com.newterrastudios.djinn.go.data

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class ServersTest {
    private val pin = "ab12cd34ef567890".repeat(4)

    @Test
    fun `changing the scheme keeps the host and port`() {
        assertEquals("https://192.168.1.20:8067", withScheme("http://192.168.1.20:8067", "https"))
        assertEquals("http://192.168.1.20:8067", withScheme("https://192.168.1.20:8067", "http"))
        assertEquals("https://[fd00::1]:8067", withScheme("http://[fd00::1]:8067", "https"))
        assertNull(withScheme("not an address", "https"))
    }

    @Test
    fun `a QR code can carry the certificate`() {
        assertEquals(pin, pinFromQr("http://10.0.0.2:8067#fp=$pin"))
        assertNull(pinFromQr("http://10.0.0.2:8067"))
        assertNull(pinFromQr("http://10.0.0.2:8067#fp=short"))
        assertNull(pinFromQr("http://10.0.0.2:8067#fp=" + "Z".repeat(64)))
    }

    @Test
    fun `the address in a QR code still reads as an address`() {
        assertEquals("http://10.0.0.2:8067", serverAddress("http://10.0.0.2:8067#fp=$pin"))
    }

    @Test
    fun `security codes match the server's form`() {
        assertEquals("AB12-CD34-EF56-7890", securityCode(pin))
    }

    @Test
    fun `fingerprints are SHA-256 in hex`() {
        assertEquals(
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            fingerprint(ByteArray(0)),
        )
    }

    @Test
    fun `each account on each server has its own songs`() {
        val sam = SavedServer("https://10.0.0.2:8067", id = "dorm", accountId = 1)
        val alex = sam.copy(accountId = 2)
        val elsewhere = SavedServer("https://10.0.0.3:8067", id = "attic", accountId = 1)
        assertEquals("dorm#1", scopeOf(sam))
        assertEquals("dorm#2", scopeOf(alex))
        assertEquals("attic#1", scopeOf(elsewhere))
        assertNull(scopeOf(sam.copy(accountId = null)))
    }

    @Test
    fun `a server's songs survive it getting a new address`() {
        val before = SavedServer("https://10.0.0.2:8067", id = "dorm", accountId = 1)
        assertEquals(scopeOf(before), scopeOf(before.copy(address = "https://10.0.0.99:8067")))
    }

    @Test
    fun `an old server without an id goes by its address`() {
        assertEquals("http://10.0.0.2:8067#1", scopeOf(SavedServer("http://10.0.0.2:8067", accountId = 1)))
    }

    @Test
    fun `one server's scope isn't the start of another's`() {
        val first = SavedServer("https://a", id = "dorm", accountId = 1)
        val second = SavedServer("https://b", id = "dorm2", accountId = 1)
        assertEquals(false, scopeOf(second)!!.startsWith(serverScope(first)))
    }

    @Test
    fun `servers are found by host and port`() {
        val servers = listOf(
            SavedServer("https://10.0.0.2:8067", pin = "first"),
            SavedServer("https://10.0.0.2:9000", pin = "second"),
        )
        assertEquals("second", serverAt(servers, "10.0.0.2", 9000)?.pin)
        assertNull(serverAt(servers, "10.0.0.3", 8067))
    }
}
