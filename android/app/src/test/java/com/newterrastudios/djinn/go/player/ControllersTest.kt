package com.newterrastudios.djinn.go.player

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class ControllersTest {
    @Test
    fun onlyTheSystemsOwnPlayersSeeTheLibrary() {
        assertTrue(mayBrowse("com.google.android.projection.gearhead"))
        assertTrue(mayBrowse("com.android.bluetooth"))
        assertFalse(mayBrowse("com.example.someapp"))
        assertFalse(mayBrowse(""))
    }
}
