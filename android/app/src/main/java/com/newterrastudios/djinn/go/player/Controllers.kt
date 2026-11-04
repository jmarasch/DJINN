package com.newterrastudios.djinn.go.player

import androidx.annotation.OptIn
import androidx.media3.common.util.UnstableApi
import androidx.media3.session.MediaSession

/** Android's own players of other apps' sessions: voice, watches, Bluetooth, the system. */
@OptIn(UnstableApi::class)
private val SYSTEM_PLAYERS = setOf(
    "com.android.bluetooth",
    "com.android.systemui",
    "com.google.android.bluetooth",
    "com.google.android.googlequicksearchbox",
    "com.google.android.projection.gearhead",
    "com.google.android.wearable.app",
    "com.samsung.android.app.watchmanager",
    MediaSession.ControllerInfo.LEGACY_CONTROLLER_PACKAGE_NAME,
)

/** Whether this app sees the library, on top of the ones Android vouches for. */
internal fun mayBrowse(packageName: String): Boolean = packageName in SYSTEM_PLAYERS
