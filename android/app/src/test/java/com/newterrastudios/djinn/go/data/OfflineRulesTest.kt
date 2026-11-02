package com.newterrastudios.djinn.go.data

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class OfflineRulesTest {
    private val day = 24L * 60 * 60 * 1000

    @Test
    fun `kept songs go after 30 days without the server`() {
        val now = 1_000 * day
        assertFalse(tooLongOffline(now - 29 * day, now))
        assertTrue(tooLongOffline(now - 31 * day, now))
    }

    @Test
    fun `never having reached the server isn't being away`() {
        assertFalse(tooLongOffline(0, 1_000 * day))
    }

    @Test
    fun `small downloads just start`() {
        assertNull(largeDownloadWarning(50_000_000, 128, canConvert = true))
        assertNull(largeDownloadWarning(50_000_000, 128, canConvert = false))
        assertNull(largeDownloadWarning(400_000_000, 0, canConvert = true))
    }

    @Test
    fun `no converter warns sooner and says why`() {
        val warning = largeDownloadWarning(150_000_000, 128, canConvert = false)
        assertNotNull(warning)
        assertTrue(warning!!, "150 MB" in warning && "isn't converting" in warning)
        assertNull(largeDownloadWarning(150_000_000, 128, canConvert = true))
    }

    @Test
    fun `large downloads say the size`() {
        assertEquals("Full quality: 2.5 GB.", largeDownloadWarning(2_500_000_000, 0, canConvert = true))
        assertTrue(largeDownloadWarning(700_000_000, 128, canConvert = true)!!.startsWith("Up to 700 MB"))
    }
}
