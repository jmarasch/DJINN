package com.newterrastudios.djinn.go.data

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class AddressTest {
    @Test
    fun `bare hosts get http and GO's port`() {
        assertEquals("http://192.168.1.20:8067", serverAddress("192.168.1.20"))
        assertEquals("http://djinn.local:8067", serverAddress(" djinn.local/ "))
        assertEquals("http://192.168.1.20:9000", serverAddress("192.168.1.20:9000"))
    }

    @Test
    fun `a scheme keeps its own default port`() {
        assertEquals("https://music.example.com", serverAddress("https://music.example.com/"))
        assertEquals("http://music.example.com", serverAddress("http://music.example.com"))
        assertEquals("https://music.example.com:8443", serverAddress("HTTPS://music.example.com:8443"))
    }

    @Test
    fun `a path is dropped`() {
        assertEquals("http://192.168.1.20:8067", serverAddress("http://192.168.1.20:8067/go.html"))
    }

    @Test
    fun `nonsense is refused`() {
        assertNull(serverAddress(""))
        assertNull(serverAddress("   "))
        assertNull(serverAddress("two words"))
        assertNull(serverAddress("ftp://example.com"))
    }

    @Test
    fun `https and home addresses are safe for a password`() {
        assertTrue(safeToSignIn("https://music.example.com"))
        assertTrue(safeToSignIn("http://192.168.1.20:8067"))
        assertTrue(safeToSignIn("http://10.0.2.2:8067"))
        assertTrue(safeToSignIn("http://172.16.0.1:8067"))
        assertTrue(safeToSignIn("http://100.101.102.103:8067"))
        assertTrue(safeToSignIn("http://djinn.local:8067"))
    }

    @Test
    fun `plain http anywhere else is not`() {
        assertFalse(safeToSignIn("http://music.example.com:8067"))
        assertFalse(safeToSignIn("http://8.8.8.8:8067"))
        assertFalse(safeToSignIn("http://172.32.0.1:8067"))
        assertFalse(safeToSignIn("http://100.128.0.1:8067"))
        assertFalse(safeToSignIn("http://192.169.1.1:8067"))
    }
}
