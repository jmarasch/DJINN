package com.newterrastudios.djinn.go.player

import android.content.Context
import android.media.AudioAttributes
import android.media.AudioFocusRequest
import android.media.AudioManager
import android.os.Build
import android.os.Handler
import android.os.Looper
import androidx.media3.common.Player

/** What the music does when another app asks it to turn down so it can talk. */
enum class TalkOver(val label: String) {
    LOWER("Lower the music"),
    KEEP("Keep playing"),
    PAUSE("Pause"),
    ;

    companion object {
        fun named(name: String?) = entries.firstOrNull { it.name == name } ?: LOWER
    }
}

enum class FocusAction { NOTHING, STOP, WAIT, CARRY_ON }

/**
 * What to do about a change in audio focus. Only a request to turn down
 * depends on the choice: a call you answer asks for the sound outright.
 */
fun focusAction(change: Int, choice: TalkOver): FocusAction = when (change) {
    AudioManager.AUDIOFOCUS_LOSS -> FocusAction.STOP
    AudioManager.AUDIOFOCUS_LOSS_TRANSIENT -> FocusAction.WAIT
    AudioManager.AUDIOFOCUS_LOSS_TRANSIENT_CAN_DUCK ->
        if (choice == TalkOver.PAUSE) FocusAction.WAIT else FocusAction.NOTHING
    AudioManager.AUDIOFOCUS_GAIN -> FocusAction.CARRY_ON
    else -> FocusAction.NOTHING
}

/**
 * Holds audio focus for the player when the choice isn't [TalkOver.LOWER],
 * which is the player's own handling. Android turns an app down by itself
 * unless the app says it will deal with that, which is what this does.
 * Called on the player's thread, the main one.
 */
class TalkOverFocus(context: Context, private val player: Player) : Player.Listener {
    private val audio = context.getSystemService(Context.AUDIO_SERVICE) as AudioManager
    private val changes = AudioManager.OnAudioFocusChangeListener(::changed)
    private val request = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
        AudioFocusRequest.Builder(AudioManager.AUDIOFOCUS_GAIN)
            .setAudioAttributes(
                AudioAttributes.Builder()
                    .setUsage(AudioAttributes.USAGE_MEDIA)
                    .setContentType(AudioAttributes.CONTENT_TYPE_MUSIC)
                    .build(),
            )
            .setWillPauseWhenDucked(true)
            .setOnAudioFocusChangeListener(changes, Handler(Looper.getMainLooper()))
            .build()
    } else {
        null
    }
    private var choice = TalkOver.LOWER
    private var held = false
    private var waiting = false

    fun choose(choice: TalkOver) {
        this.choice = choice
        follow()
    }

    fun release() {
        waiting = false
        letGo()
    }

    override fun onPlayWhenReadyChanged(playWhenReady: Boolean, reason: Int) {
        // Playing again by hand ends the wait.
        if (playWhenReady) waiting = false
        follow()
    }

    override fun onPlaybackStateChanged(playbackState: Int) = follow()

    private fun follow() {
        val wantsSound = choice != TalkOver.LOWER && player.playWhenReady &&
            (player.playbackState == Player.STATE_BUFFERING || player.playbackState == Player.STATE_READY)
        when {
            wantsSound && !held -> take()
            !wantsSound && held && !(waiting && choice != TalkOver.LOWER) -> letGo()
        }
    }

    private fun take() {
        val answer = if (request != null) {
            audio.requestAudioFocus(request)
        } else {
            @Suppress("DEPRECATION")
            audio.requestAudioFocus(changes, AudioManager.STREAM_MUSIC, AudioManager.AUDIOFOCUS_GAIN)
        }
        if (answer == AudioManager.AUDIOFOCUS_REQUEST_GRANTED) held = true else player.pause()
    }

    private fun letGo() {
        held = false
        if (request != null) {
            audio.abandonAudioFocusRequest(request)
        } else {
            @Suppress("DEPRECATION")
            audio.abandonAudioFocus(changes)
        }
    }

    private fun changed(change: Int) {
        when (focusAction(change, choice)) {
            FocusAction.STOP -> {
                waiting = false
                player.pause()
                letGo()
            }
            FocusAction.WAIT -> if (player.playWhenReady) {
                waiting = true
                player.pause()
            }
            FocusAction.CARRY_ON -> if (waiting) {
                waiting = false
                player.play()
            }
            FocusAction.NOTHING -> Unit
        }
    }
}
