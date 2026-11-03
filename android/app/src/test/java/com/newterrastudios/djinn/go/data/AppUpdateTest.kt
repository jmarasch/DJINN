package com.newterrastudios.djinn.go.data

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import java.io.File

class AppUpdateTest {
    private val release = AppRelease("2026.11.02", 20261102, "abc", 1)

    @Test
    fun `a newer version is offered`() {
        assertEquals(release, offeredUpdate(release, 20261101))
    }

    @Test
    fun `the same or an older version isn't`() {
        assertNull(offeredUpdate(release, 20261102))
        assertNull(offeredUpdate(release, 20261103))
    }

    @Test
    fun `a server without the app offers nothing`() {
        assertNull(offeredUpdate(null, 1))
    }

    @Test
    fun `checksums are SHA-256 in hex`() {
        val file = File.createTempFile("djinn", "test")
        try {
            file.writeText("abc")
            assertEquals(
                "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
                sha256Of(file),
            )
        } finally {
            file.delete()
        }
    }
}
