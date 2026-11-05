package com.newterrastudios.djinn.go.player

import android.media.AudioManager
import org.junit.Assert.assertEquals
import org.junit.Test

class TalkOverTest {
    private val duck = AudioManager.AUDIOFOCUS_LOSS_TRANSIENT_CAN_DUCK

    @Test
    fun onlyTurningDownDependsOnTheChoice() {
        assertEquals(FocusAction.NOTHING, focusAction(duck, TalkOver.KEEP))
        assertEquals(FocusAction.WAIT, focusAction(duck, TalkOver.PAUSE))
    }

    @Test
    fun anAnsweredCallPausesWhateverTheChoice() {
        for (choice in TalkOver.entries) {
            assertEquals(FocusAction.WAIT, focusAction(AudioManager.AUDIOFOCUS_LOSS_TRANSIENT, choice))
            assertEquals(FocusAction.STOP, focusAction(AudioManager.AUDIOFOCUS_LOSS, choice))
            assertEquals(FocusAction.CARRY_ON, focusAction(AudioManager.AUDIOFOCUS_GAIN, choice))
        }
    }

    @Test
    fun anUnknownNameIsTheDefault() {
        assertEquals(TalkOver.LOWER, TalkOver.named(null))
        assertEquals(TalkOver.LOWER, TalkOver.named("LOUDER"))
        assertEquals(TalkOver.PAUSE, TalkOver.named("PAUSE"))
    }
}
