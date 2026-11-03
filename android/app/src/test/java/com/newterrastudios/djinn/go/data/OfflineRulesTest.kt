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
    fun `kept songs lock after 30 days without the server, unless told otherwise`() {
        val now = 1_000 * day
        val rules = KeepRules()
        assertEquals(WhenAway.Nothing, whenAway(now - 29 * day, now, rules))
        assertEquals(WhenAway.Lock, whenAway(now - 31 * day, now, rules))
    }

    @Test
    fun `the admin chooses the time and what happens`() {
        val now = 1_000 * day
        assertEquals(WhenAway.Delete, whenAway(now - 8 * day, now, KeepRules(away = "delete", awayDays = 7)))
        assertEquals(WhenAway.Nothing, whenAway(now - 6 * day, now, KeepRules(away = "delete", awayDays = 7)))
        assertEquals(WhenAway.Nothing, whenAway(now - 900 * day, now, KeepRules(away = "off")))
        assertEquals(WhenAway.Lock, whenAway(now - 91 * day, now, KeepRules(away = "lock", awayDays = 90)))
    }

    @Test
    fun `a rule this version doesn't know locks rather than deletes`() {
        val now = 1_000 * day
        assertEquals(WhenAway.Lock, whenAway(now - 31 * day, now, KeepRules(away = "something new")))
    }

    @Test
    fun `never having reached the server isn't being away`() {
        assertEquals(WhenAway.Nothing, whenAway(0, 1_000 * day, KeepRules()))
    }

    @Test
    fun `a song from another server knows where it's from`() {
        val mediaId = keptMediaId("dorm#3", 42)
        assertEquals("dorm#3" to 42L, parseKeptMediaId(mediaId))
        assertEquals("https://10.0.0.2:8067#3" to 7L, parseKeptMediaId(keptMediaId("https://10.0.0.2:8067#3", 7)))
    }

    @Test
    fun `an ordinary song isn't from another server`() {
        assertNull(parseKeptMediaId("42"))
        assertNull(parseKeptMediaId("kept|"))
        assertNull(parseKeptMediaId("kept|no-number"))
        assertNull(parseKeptMediaId("kept|dorm#3|not-a-number"))
    }

    @Test
    fun `songs from elsewhere only play with a sign-in and within the rules`() {
        assertFalse(lockedElsewhere(signedIn = true, awayLocked = false, away = WhenAway.Nothing))
        assertTrue(lockedElsewhere(signedIn = false, awayLocked = false, away = WhenAway.Nothing))
        assertTrue(lockedElsewhere(signedIn = true, awayLocked = true, away = WhenAway.Nothing))
        assertTrue(lockedElsewhere(signedIn = true, awayLocked = false, away = WhenAway.Lock))
        assertTrue(lockedElsewhere(signedIn = true, awayLocked = false, away = WhenAway.Delete))
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
